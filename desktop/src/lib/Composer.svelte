<script lang="ts">
  import { app } from '$lib/state.svelte';
  import { emx } from '$lib/emx';

  const start = app.composing ?? {};
  let from = $state(app.me?.sendFrom.find((s) => s.primary && s.address.toLowerCase() === app.accountName.toLowerCase())?.address ?? app.me?.sendFrom[0]?.address ?? '');
  let to = $state(start.to ?? '');
  let cc = $state('');
  let showCc = $state(false);
  let subject = $state(start.subject ?? '');
  let text = $state(start.text ?? '');
  let files = $state<{ filename: string; contentType: string; data: string }[]>([]);
  let busy = $state(false);

  async function attach(e: Event): Promise<void> {
    const input = e.currentTarget as HTMLInputElement;
    for (const f of input.files ?? []) {
      if (f.size > 25 * 1024 * 1024) {
        app.fail(`${f.name} is larger than 25 MB`);
        continue;
      }
      const buf = new Uint8Array(await f.arrayBuffer());
      let bin = '';
      for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000));
      files = [...files, { filename: f.name, contentType: f.type || 'application/octet-stream', data: btoa(bin) }];
    }
    input.value = '';
  }

  async function send(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    busy = true;
    try {
      const n = await emx.send({
        from,
        to,
        cc: showCc ? cc : '',
        subject,
        text,
        inReplyTo: start.inReplyTo,
        references: start.references,
        attachments: files
      });
      app.say(`Sent to ${n} recipient${n === 1 ? '' : 's'}`);
      app.composing = null;
    } catch (err) {
      app.fail(err);
    } finally {
      busy = false;
    }
  }

  let form: HTMLFormElement;
  function key(e: KeyboardEvent): void {
    if (e.key === 'Escape') app.composing = null;
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') form.requestSubmit();
  }
</script>

<svelte:window onkeydown={key} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (app.composing = null)}>
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
      <button type="button" class="btn quiet" onclick={() => (app.composing = null)}>Discard</button>
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
