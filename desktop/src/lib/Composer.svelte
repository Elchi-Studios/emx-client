<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { app } from '$lib/state.svelte';
  import { emx, newKey } from '$lib/emx';
  import { fits, normalizeRecipients } from '$lib/mail';
  import { size } from '$lib/state.svelte';

  const start = app.composing ?? {};
  let from = $state(app.me?.sendFrom.find((s) => s.primary && s.address.toLowerCase() === app.accountName.toLowerCase())?.address ?? app.me?.sendFrom[0]?.address ?? '');
  let to = $state(start.to ?? '');
  let cc = $state('');
  let showCc = $state(false);
  let subject = $state(start.subject ?? '');
  let text = $state(start.text ?? '');
  let files = $state<{ filename: string; contentType: string; data: string; size: number }[]>([]);
  let busy = $state(false);
  // One key for this message, however often Send is pressed. After a
  // timeout the message may have gone out; sending again with the same
  // key sends it if it did not, and not a second time if it did.
  const idempotencyKey = newKey();

  // The largest message the service takes, attachments and text
  // together, as mail encodes them.
  const max = app.me?.limits?.maxMessageBytes ?? 0;

  async function attach(e: Event): Promise<void> {
    const input = e.currentTarget as HTMLInputElement;
    for (const f of input.files ?? []) {
      // Checked before the file is read: a message over the limit is
      // refused by the service only after the whole upload.
      if (!fits([...files.map((x) => x.size), f.size], text, max)) {
        app.fail(
          `${f.name} (${size(f.size)}) does not fit: a message can be at most ${size(max)}, and attachments take about a third more as mail.`
        );
        continue;
      }
      const buf = new Uint8Array(await f.arrayBuffer());
      let bin = '';
      for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000));
      files = [...files, { filename: f.name, contentType: f.type || 'application/octet-stream', data: btoa(bin), size: f.size }];
    }
    input.value = '';
  }

  async function send(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    busy = true;
    try {
      // Written the way the service reads them: `;` between addresses,
      // as Outlook writes them, and names with `[`, `:` or `;` failed.
      const draft = {
        from,
        to: normalizeRecipients(to),
        cc: showCc ? normalizeRecipients(cc) : '',
        subject,
        text,
        inReplyTo: start.inReplyTo,
        references: start.references,
        attachments: files.map(({ filename, contentType, data }) => ({ filename, contentType, data }))
      };
      const n = await emx.send(draft, idempotencyKey);
      app.say(`Sent to ${n} recipient${n === 1 ? '' : 's'}`);
      app.composing = null;
    } catch (err) {
      app.fail(err);
    } finally {
      busy = false;
    }
  }

  // Whether anything was written or changed since the composer opened.
  function changed(): boolean {
    return to !== (start.to ?? '') || cc !== '' || subject !== (start.subject ?? '') || text !== (start.text ?? '') || files.length > 0;
  }

  // Closes the composer, asking first when that would throw text away.
  async function close(): Promise<void> {
    if (busy) return;
    if (changed() && !(await app.ask('Discard this message? What you wrote is lost.', 'Discard'))) return;
    app.composing = null;
  }

  let form: HTMLFormElement;
  function key(e: KeyboardEvent): void {
    if (app.asking) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') form.requestSubmit();
  }
</script>

<svelte:window onkeydown={key} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <form class="sheet" onsubmit={send} bind:this={form}>
    <div class="row">
      <label for="from">From</label>
      <select id="from" class="field" bind:value={from}>
        {#each app.me?.sendFrom ?? [] as s (s.address)}<option value={s.address}>{s.name ? `${s.name} <${s.address}>` : s.address}</option>{/each}
      </select>
    </div>
    <div class="row">
      <label for="to">To</label>
      <input id="to" class="field" bind:value={to} placeholder="name@example.ch, another@example.ch" required />
      {#if !showCc}<button type="button" class="btn quiet sm" onclick={() => (showCc = true)}>Cc</button>{/if}
    </div>
    {#if showCc}
      <div class="row"><label for="cc">Cc</label><input id="cc" class="field" bind:value={cc} /></div>
    {/if}
    <div class="row">
      <label for="subject">Subject</label>
      <input id="subject" class="field" bind:value={subject} />
    </div>
    <textarea class="field" bind:value={text} placeholder="Write here" rows="12"></textarea>
    {#if files.length}
      <div class="files">
        {#each files as f, i (f.filename + i)}
          <span class="file">{f.filename} <button type="button" class="x" onclick={() => (files = files.filter((_, j) => j !== i))}>×</button></span>
        {/each}
      </div>
    {/if}
    <div class="foot">
      <label class="btn">Attach <input type="file" multiple hidden onchange={attach} /></label>
      <span class="muted small">Ctrl+Enter sends, Esc closes</span>
      <span class="grow"></span>
      <button type="button" class="btn quiet" onclick={close}>Discard</button>
      <button class="btn primary" disabled={busy || !to}>{busy ? 'Sending' : 'Send'}</button>
    </div>
  </form>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: grid;
    place-items: center;
    z-index: 40;
  }
  .sheet {
    width: min(760px, 94vw);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .row label {
    width: 60px;
    color: var(--muted);
    font-size: 13px;
  }
  .files {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .file {
    padding: 3px 8px;
    border-radius: var(--r);
    background: var(--well);
    font-size: 13px;
  }
  .x {
    border: 0;
    background: none;
    cursor: pointer;
    color: var(--muted);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .grow {
    flex: 1;
  }
</style>
