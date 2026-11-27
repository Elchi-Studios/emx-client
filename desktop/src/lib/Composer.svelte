<script lang="ts">
  import { untrack } from 'svelte';
  import { app } from '$lib/state.svelte';
  import { emx, newKey } from '$lib/emx';

  const start = app.composing ?? {};
  let from = $state(app.me?.sendFrom.find((s) => s.primary && s.address.toLowerCase() === app.accountName.toLowerCase())?.address ?? app.me?.sendFrom[0]?.address ?? '');
  let to = $state(start.to ?? '');
  let cc = $state('');
  let showCc = $state(false);
  let subject = $state(start.subject ?? '');
  let text = $state(start.text ?? '');
  let files = $state<{ filename: string; contentType: string; data: string }[]>([]);
  let busy = $state(false);
  // One key for this message, however often Send is pressed. After a
  // timeout the message may have gone out; sending again with the same
  // key sends it if it did not, and not a second time if it did.
  const idempotencyKey = newKey();

  // The draft is kept on the server as it is typed, as the web client
  // does, so a message begun here can be finished on another device. It
  // is saved into an account at a version: the server says which account
  // and answers each save with the next version. Saves go one at a time,
  // in order, and a draft is deleted only after the last of them.
  const draftId = crypto.randomUUID();
  let draftAccount = 'me';
  let draftVersion = 0;
  let draftGone = false;
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let chain: Promise<void> = Promise.resolve();
  let note = $state('');

  function draftBody(): Record<string, unknown> {
    const b: Record<string, unknown> = { from, to, subject, text };
    if (showCc && cc) b.cc = cc;
    if (start.inReplyTo) b.inReplyTo = start.inReplyTo;
    if (start.references?.length) b.references = start.references;
    return b;
  }

  function scheduleSave(): void {
    if (busy || draftGone || !changed()) return;
    const body = draftBody();
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => void flush(body), 2000);
  }

  // The fields are watched, and a change starts the two seconds again.
  $effect(() => {
    void [from, to, cc, subject, text];
    untrack(scheduleSave);
  });

  function flush(body: Record<string, unknown>): Promise<void> {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = null;
    chain = chain.then(() => put(body)).catch(() => {
      note = 'Draft not saved';
    });
    return chain;
  }

  async function put(body: Record<string, unknown>): Promise<void> {
    for (let attempt = 0; attempt < 3 && !draftGone; attempt++) {
      const r = await emx.saveDraft(draftAccount, draftId, draftVersion, body);
      if (r.status === 'saved') {
        draftAccount = r.draft.account;
        draftVersion = r.draft.version;
        note = 'Draft saved';
        return;
      }
      if (r.status === 'sent') {
        draftGone = true;
        note = '';
        app.fail('This draft was sent from another device.');
        return;
      }
      // Somebody saved a newer version: nobody overwrites it unseen.
      draftAccount = r.draft.account;
      draftVersion = r.draft.version;
      if (await app.ask('This draft was changed on another device. Replace it with what is written here?', 'Replace')) continue;
      const theirs = r.draft.body as Record<string, string | undefined>;
      to = theirs.to ?? '';
      subject = theirs.subject ?? '';
      text = theirs.text ?? '';
      if (theirs.cc) {
        cc = theirs.cc;
        showCc = true;
      }
      return;
    }
  }

  // Removes the draft from the server once its last save is done.
  async function dropDraft(): Promise<void> {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = null;
    draftGone = true;
    await chain;
    if (draftVersion > 0) {
      try {
        await emx.deleteDraft(draftAccount, draftId, 0);
      } catch {
        // Left on the server, where it is deleted from the Drafts folder.
      }
    }
  }

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
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = null;
    try {
      const draft = {
        from,
        to,
        cc: showCc ? cc : '',
        subject,
        text,
        inReplyTo: start.inReplyTo,
        references: start.references,
        attachments: files
      };
      const n = await emx.send(draft, idempotencyKey);
      app.say(`Sent to ${n} recipient${n === 1 ? '' : 's'}`);
      app.composing = null;
      void dropDraft();
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
    void dropDraft();
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
      <span class="muted small">Ctrl+Enter sends, Esc closes{note ? `. ${note}` : ''}</span>
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
