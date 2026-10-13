<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { app, size, type ComposerFile } from '$lib/state.svelte';
  import { emx, type Draft, type Hold, type OnVersion, type SavedDraft, type Snippet } from '$lib/emx';
  import { countRecipients, fits, normalizeRecipients } from '$lib/mail';
  import {
    bodyFor,
    fieldsOf,
    fillSnippet,
    laterChoices,
    localInput,
    pickedTime,
    recipientName,
    sameFields,
    SendKey,
    snippetAsked,
    snippetsFor,
    undoSeconds,
    type DraftFields
  } from '$lib/compose';

  const start = app.composing ?? {};
  // A draft saved on EMX: its id, the account it belongs to, the version
  // it was loaded or last saved at, and its body then, whose fields of
  // the web client's own are kept.
  let saved = $state(start.draft ? { ...start.draft } : null);
  const loaded = start.draft ? fieldsOf(start.draft.body) : null;
  const primary =
    app.me?.sendFrom.find((s) => s.primary && s.address.toLowerCase() === app.accountName.toLowerCase())?.address ??
    app.me?.sendFrom[0]?.address ??
    '';
  let from = $state(loaded?.from || primary);
  let to = $state(loaded?.to ?? start.to ?? '');
  let cc = $state(loaded?.cc ?? '');
  let bcc = $state(loaded?.bcc ?? '');
  let showCc = $state(!!(loaded?.cc || loaded?.bcc));
  let subject = $state(loaded?.subject ?? start.subject ?? '');
  let text = $state(loaded?.text ?? start.text ?? '');
  const inReplyTo = loaded?.inReplyTo ?? start.inReplyTo;
  const references = loaded?.references ?? start.references;
  let files = $state<ComposerFile[]>(start.files ?? []);
  let busy = $state(false);
  let saving = $state(false);
  let laterOpen = $state(false);
  let picked = $state('');
  // The fields as last saved on EMX, or as the composer opened.
  let savedFields: DraftFields = current();
  // One key per request: the same while the message stays the same, so
  // that Send pressed again after a timeout cannot send it twice, and a
  // new one for a message changed meanwhile.
  const sendKey = new SendKey();

  // The largest message the service takes, attachments and text
  // together, as mail encodes them, and the most recipients of one.
  const max = app.me?.limits?.maxMessageBytes ?? 0;
  const maxRecipients = app.me?.limits?.maxRecipients ?? 0;

  function current(): DraftFields {
    return { from, to, cc, bcc, subject, text, inReplyTo, references };
  }

  // Whether anything changed since the draft was last saved, or since
  // the composer opened.
  function changed(): boolean {
    return !sameFields(current(), savedFields) || (files.length > 0 && !saved);
  }

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

  // --- Saving --------------------------------------------------------------

  /// Asks what to do with a draft somebody changed since this composer
  /// loaded it: save this one over theirs, having seen that they did, or
  /// take theirs. Answers true when this one is to be saved.
  async function resolve(newer: SavedDraft): Promise<boolean> {
    const who = newer.editedBy?.name ?? 'Somebody';
    const mine = await app.ask(
      `${who} changed this draft since you opened it. Save your version over theirs? Cancel shows theirs instead; what you wrote here is lost then.`,
      'Save mine'
    );
    if (!app.composing) return false;
    if (!mine) {
      const f = fieldsOf(newer.body);
      from = f.from || from;
      to = f.to;
      cc = f.cc;
      bcc = f.bcc;
      subject = f.subject;
      text = f.text;
      showCc = showCc || !!(f.cc || f.bcc);
      saved = { id: newer.id, account: newer.account, version: newer.version, body: newer.body ?? {} };
      savedFields = current();
    } else {
      saved = { id: newer.id, account: newer.account, version: newer.version, body: newer.body ?? {} };
    }
    return mine;
  }

  /// Somebody sent the draft: the composer closes, as nothing is left
  /// to send.
  function sentElsewhere(d: SavedDraft): void {
    app.say(`${d.sentBy?.name ?? 'Somebody'} sent this draft already.`);
    app.composing = null;
    void app.loadDrafts();
  }

  /// Saves the message as a draft on EMX, on the version it was loaded
  /// at. Answers whether it is saved; a draft somebody changed meanwhile
  /// is asked about first, and one somebody sent closes the composer.
  /// `sealed` is called instead of saying why when the person's own
  /// mailbox is sealed, whose drafts only the web client saves.
  async function save(sealed?: () => void): Promise<boolean> {
    if (saving) return false;
    saving = true;
    try {
      for (let attempt = 0; attempt < 3; attempt++) {
        const id = saved?.id ?? crypto.randomUUID();
        // A new draft goes into the person's own account, and EMX moves
        // it to the shared mailbox its From address belongs to.
        const account = saved?.account || 'me';
        const fields = current();
        const r: OnVersion<SavedDraft> = await emx.saveDraft(account, id, saved?.version ?? 0, bodyFor(saved?.body, fields));
        if (!app.composing) return false;
        if (r.outcome === 'done') {
          saved = { id: r.value.id, account: r.value.account || account, version: r.value.version, body: r.value.body ?? {} };
          savedFields = fields;
          return true;
        }
        if (r.outcome === 'sent') {
          sentElsewhere(r.value);
          return false;
        }
        if (!(await resolve(r.value))) return false;
      }
      return false;
    } catch (e) {
      if (sealed && (e as { code?: string }).code === 'sealed') sealed();
      else app.fail(e);
      return false;
    } finally {
      saving = false;
    }
  }

  async function saveNow(): Promise<void> {
    if (await save()) {
      app.say('Draft saved');
      void app.loadDrafts();
    }
  }

  // --- Sending -------------------------------------------------------------

  /// A short fingerprint of the files, so that the key changes with them
  /// without keeping their bytes twice.
  function filesPrint(): string {
    return files
      .map((f) => {
        let h = 0x811c9dc5;
        for (let i = 0; i < f.data.length; i++) h = Math.imul(h ^ f.data.charCodeAt(i), 0x01000193);
        return `${f.filename}:${f.size}:${(h >>> 0).toString(16)}`;
      })
      .join('|');
  }

  function message(): Draft {
    return {
      from,
      to: normalizeRecipients(to),
      cc: showCc ? normalizeRecipients(cc) : '',
      bcc: showCc ? normalizeRecipients(bcc) : '',
      subject,
      text,
      inReplyTo,
      references,
      attachments: files.map(({ filename, contentType, data }) => ({ filename, contentType, data }))
    };
  }

  /// Sends the message: held on EMX for the seconds Undo has, or until
  /// `later`, or at once when Undo is off. A held message is held as its
  /// saved draft's, so the draft is saved first.
  async function send(later?: Date): Promise<void> {
    if (busy) return;
    laterOpen = false;
    // Written the way the service reads them: `;` between addresses,
    // as Outlook writes them, and names with `[`, `:` or `;` failed.
    const m = message();
    const n = countRecipients(m.to, m.cc ?? '', m.bcc ?? '');
    if (n === 0) {
      app.fail('Name at least one recipient.');
      return;
    }
    if (maxRecipients && n > maxRecipients) {
      app.fail(`A message can go to at most ${maxRecipients} recipients, To, Cc and Bcc together; this one names ${n}.`);
      return;
    }
    const undo = later ? 0 : undoSeconds(app.me?.prefs);
    const hold: Hold = later ? { sendAt: later.toISOString() } : undo > 0 ? { undoSeconds: undo } : {};
    busy = true;
    try {
      if (!later && undo === 0 && !saved) {
        // At once, as before drafts were kept on EMX.
        const key = sendKey.for(JSON.stringify({ to: 'send', m: { ...m, attachments: filesPrint() } }));
        const count = await emx.send(m, key);
        app.say(`Sent to ${count} recipient${count === 1 ? '' : 's'}`);
        app.composing = null;
        return;
      }
      if (!saved || changed()) {
        // A sealed mailbox's own drafts are sealed by the web client, so
        // its messages go at once from here, without Undo.
        let sealed = false;
        const ok = await save(() => (sealed = true));
        if (sealed && !later && !saved) {
          const key = sendKey.for(JSON.stringify({ to: 'send', m: { ...m, attachments: filesPrint() } }));
          const count = await emx.send(m, key);
          app.say(`Sent to ${count} recipient${count === 1 ? '' : 's'}, at once: a sealed mailbox's messages are held in the web client only`);
          app.composing = null;
          return;
        }
        if (sealed) {
          app.fail('This mailbox is sealed: its drafts, and so Send later, are kept by the web client only.');
          return;
        }
        if (!ok) return;
      }
      for (let attempt = 0; attempt < 3 && saved; attempt++) {
        const s = saved;
        const key = sendKey.for(JSON.stringify({ to: `${s.account}/${s.id}`, v: s.version, m: { ...m, attachments: filesPrint() }, hold }));
        // The Undo note lasts as long as EMX holds the message, counted
        // from the request, whatever the clocks say.
        const asked = Date.now();
        const r = await emx.sendDraft(s.account, s.id, s.version, m, hold, key);
        if (r.outcome === 'sent') {
          sentElsewhere(r.value);
          return;
        }
        if (r.outcome === 'changed') {
          // Changed while it was sent: shown before anything goes.
          if (!(await resolve(r.value))) return;
          if (!(await save())) return;
          continue;
        }
        const sent = r.value;
        app.composing = null;
        void app.loadDrafts();
        if (sent.held && sent.outbox && hold.undoSeconds) {
          app.offerUndo(sent.outbox.account || s.account, sent.outbox.id, hold.undoSeconds, asked);
        } else if (sent.held && sent.outbox) {
          app.say(`Will be sent ${new Date(sent.outbox.sendAt).toLocaleString()}`);
        } else {
          app.say(`Sent to ${sent.recipients} recipient${sent.recipients === 1 ? '' : 's'}`);
        }
        return;
      }
    } catch (err) {
      if ((err as { code?: string }).code === 'idempotency_mismatch') sendKey.forget();
      app.fail(err);
    } finally {
      busy = false;
    }
  }

  function sendPicked(): void {
    const at = pickedTime(picked, new Date());
    if (!at) {
      app.fail('Pick a time ahead, within a year.');
      return;
    }
    void send(at);
  }

  // --- Closing -------------------------------------------------------------

  // Closes the composer, asking first when that would throw text away.
  // A saved draft stays as it was last saved.
  async function close(): Promise<void> {
    if (busy || saving) return;
    if (changed()) {
      const what = saved ? 'Discard the changes? The draft stays as it was last saved.' : 'Discard this message? What you wrote is lost.';
      if (!(await app.ask(what, 'Discard'))) return;
    }
    app.composing = null;
  }

  /// Deletes the saved draft, at the version this composer has, so that
  /// a colleague's change made meanwhile is not thrown away unseen.
  async function remove(): Promise<void> {
    if (!saved || busy || saving) return;
    if (!(await app.ask('Delete this draft? Nobody can open it afterwards.', 'Delete'))) return;
    try {
      const r = await emx.deleteDraft(saved.account, saved.id, saved.version);
      if (r.outcome === 'sent') {
        sentElsewhere(r.value);
        return;
      }
      if (r.outcome === 'changed') {
        app.say(`${r.value.editedBy?.name ?? 'Somebody'} changed this draft meanwhile; it was not deleted.`);
        await resolve(r.value);
        return;
      }
      app.composing = null;
      app.say('Draft deleted');
      void app.loadDrafts();
    } catch (e) {
      app.fail(e);
    }
  }

  // --- Snippets ------------------------------------------------------------

  let snippets = $state<{ name: string; text: string }[]>([]);
  let asked = $state<{ start: number; query: string } | null>(null);
  let picking = $state(false);
  let area: HTMLTextAreaElement;

  /// The snippets of the person and of the shared mailbox the message is
  /// written from, loaded once per From address.
  async function loadSnippets(address: string): Promise<void> {
    const accounts = [...new Set(['me', app.accountOf(address)])];
    const out: { name: string; text: string }[] = [];
    for (const a of accounts) {
      try {
        const list = await emx.snippets(a);
        for (const s of list.snippets as Snippet[]) {
          const name = typeof s.body?.name === 'string' ? s.body.name : '';
          const t = typeof s.body?.text === 'string' ? s.body.text : '';
          if (name) out.push({ name, text: t });
        }
      } catch {
        // A mailbox whose snippets cannot be read offers none.
      }
    }
    if (address === from) snippets = out;
  }
  $effect(() => {
    void loadSnippets(from);
  });

  const offered = $derived(snippetsFor(snippets, asked?.query ?? '').slice(0, 8));

  function typed(): void {
    const before = text.slice(0, area?.selectionStart ?? text.length);
    asked = snippetAsked(before);
  }

  /// Puts a snippet in at the caret, in place of `;;` and what was typed
  /// after it, with the first recipient's name and the person's filled
  /// in.
  function insert(s: { name: string; text: string }): void {
    const r = recipientName(to);
    const filled = fillSnippet(s.text, { firstName: r.firstName, name: r.name, myName: app.me?.name ?? '' });
    const caret = area?.selectionStart ?? text.length;
    const from_ = asked ? asked.start : caret;
    text = text.slice(0, from_) + filled + text.slice(caret);
    asked = null;
    picking = false;
    const at = from_ + filled.length;
    queueMicrotask(() => {
      area?.focus();
      area?.setSelectionRange(at, at);
    });
  }

  function areaKey(e: KeyboardEvent): void {
    if (!asked || offered.length === 0) return;
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      insert(offered[0]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      asked = null;
    }
  }

  let form: HTMLFormElement;
  function key(e: KeyboardEvent): void {
    if (app.asking) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      if (laterOpen || picking) {
        laterOpen = false;
        picking = false;
        return;
      }
      close();
    }
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') form.requestSubmit();
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      void saveNow();
    }
  }

  function submit(e: SubmitEvent): void {
    e.preventDefault();
    void send();
  }

  const choices = laterChoices(new Date());
  // The shared mailbox a saved draft belongs to, for the line above it.
  const owner = $derived(saved ? app.accounts.find((a) => a.id === saved?.account && !a.rights.own) : undefined);
</script>

<svelte:window onkeydown={key} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <form class="sheet" onsubmit={submit} bind:this={form}>
    {#if saved}
      <div class="muted small">{owner ? `Draft of ${owner.address}, written together with its members` : 'Draft'}, saved as version {saved.version}</div>
    {/if}
    <div class="row">
      <label for="from">From</label>
      <select id="from" class="field" bind:value={from}>
        {#each app.me?.sendFrom ?? [] as s (s.address)}<option value={s.address}>{s.name ? `${s.name} <${s.address}>` : s.address}</option>{/each}
      </select>
    </div>
    <div class="row">
      <label for="to">To</label>
      <input id="to" class="field" bind:value={to} placeholder="name@example.ch, another@example.ch" required />
      {#if !showCc}<button type="button" class="btn quiet sm" onclick={() => (showCc = true)}>Cc Bcc</button>{/if}
    </div>
    {#if showCc}
      <div class="row"><label for="cc">Cc</label><input id="cc" class="field" bind:value={cc} /></div>
      <div class="row"><label for="bcc">Bcc</label><input id="bcc" class="field" bind:value={bcc} /></div>
    {/if}
    <div class="row">
      <label for="subject">Subject</label>
      <input id="subject" class="field" bind:value={subject} />
    </div>
    <div class="write">
      <textarea
        class="field"
        bind:value={text}
        bind:this={area}
        oninput={typed}
        onkeydown={areaKey}
        onclick={typed}
        placeholder="Write here; ;; and a name inserts a snippet"
        rows="12"
      ></textarea>
      {#if (asked && offered.length) || picking}
        <div class="menu snippets" role="listbox">
          {#each picking ? snippets : offered as s, i (s.name + i)}
            <button type="button" role="option" aria-selected={i === 0 && !picking} class="item" onclick={() => insert(s)}>
              <strong>{s.name}</strong>
              <span class="muted small">{s.text.split('\n').find((l) => l.trim()) ?? ''}</span>
            </button>
          {:else}
            <p class="muted small pad">No snippets. They are made in the web client, under Settings.</p>
          {/each}
        </div>
      {/if}
    </div>
    {#if files.length}
      <div class="files">
        {#each files as f, i (f.filename + i)}
          <span class="file">{f.filename} <button type="button" class="x" onclick={() => (files = files.filter((_, j) => j !== i))}>×</button></span>
        {/each}
      </div>
    {/if}
    <div class="foot">
      <label class="btn">Attach <input type="file" multiple hidden onchange={attach} /></label>
      <button type="button" class="btn quiet" onclick={() => ((picking = !picking), (asked = null))}>Snippets</button>
      <span class="grow"></span>
      {#if saved}<button type="button" class="btn quiet" onclick={remove}>Delete draft</button>{/if}
      <button type="button" class="btn quiet" onclick={close}>{saved ? 'Close' : 'Discard'}</button>
      <button type="button" class="btn" disabled={saving || busy} onclick={saveNow} title="Ctrl+S">{saving ? 'Saving' : 'Save draft'}</button>
      <div class="later">
        <button type="button" class="btn" disabled={busy || !to} onclick={() => (laterOpen = !laterOpen)}>Send later</button>
        {#if laterOpen}
          <div class="menu up">
            {#each choices as c (c.label)}
              <button type="button" class="item" onclick={() => send(c.at)}>
                <span>{c.label}</span><span class="muted small">{c.at.toLocaleString(undefined, { weekday: 'short', hour: '2-digit', minute: '2-digit' })}</span>
              </button>
            {/each}
            <div class="pick">
              <input type="datetime-local" class="field" bind:value={picked} min={localInput(new Date())} />
              <button type="button" class="btn sm" disabled={!picked} onclick={sendPicked}>Send then</button>
            </div>
          </div>
        {/if}
      </div>
      <button class="btn primary" disabled={busy || !to}>{busy ? 'Sending' : 'Send'}</button>
    </div>
    <span class="muted small">Ctrl+Enter sends, Ctrl+S saves the draft, Esc closes</span>
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
    width: min(780px, 94vw);
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
  .write {
    position: relative;
  }
  .write textarea {
    width: 100%;
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
    gap: 8px;
    flex-wrap: wrap;
  }
  .grow {
    flex: 1;
  }
  .later {
    position: relative;
  }
  .menu {
    position: absolute;
    z-index: 45;
    min-width: 260px;
    max-height: 280px;
    overflow: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.2);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }
  .menu.up {
    bottom: calc(100% + 6px);
    right: 0;
  }
  .menu.snippets {
    left: 8px;
    bottom: 8px;
    width: min(420px, 90%);
  }
  .item {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 10px;
    padding: 6px 8px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .item:hover,
  .item[aria-selected='true'] {
    background: var(--hover);
  }
  .item .small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pick {
    display: flex;
    gap: 6px;
    padding: 6px 4px 2px;
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
  .pad {
    padding: 8px;
    margin: 0;
  }
</style>
