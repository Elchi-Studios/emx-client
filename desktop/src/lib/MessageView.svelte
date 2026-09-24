<script lang="ts">
  import { app, size } from '$lib/state.svelte';
  import { emx } from '$lib/emx';

  let { onreply }: { onreply: (all: boolean) => void } = $props();
  let showRemote = $state(false);
  let html = $state('');
  let building = $state(false);

  // The HTML comes sanitised from the service: no scripts, no remote loads,
  // inline images pointing at the part endpoint. Those are fetched here
  // and put in as data URIs, because the frame has no token. Remote images
  // stay out until asked for.
  $effect(() => {
    const m = app.open;
    showRemote = false;
    html = '';
    if (!m || !m.body.html) return;
    build(m.message.id, m.body.html, false);
  });

  async function build(id: string, source: string, remote: boolean): Promise<void> {
    building = true;
    let out = source;
    const parts = [...source.matchAll(/src="\/api\/accounts\/[^"]+\/messages\/[^"]+\/parts\/([^"]+)"/g)];
    for (const hit of parts.slice(0, 30)) {
      try {
        const p = await emx.part(app.account, id, decodeURIComponent(hit[1]));
        out = out.replace(hit[0], `src="data:${p.content_type};base64,${p.base64}"`);
      } catch {
        // A part that will not load is left as it is.
      }
    }
    if (remote) out = out.replace(/data-src="/g, 'src="');
    if (app.open?.message.id !== id) return;
    html = `<!doctype html><html><head><meta charset="utf-8"><base target="_blank"><style>
      body{margin:0;padding:4px 0;font:14px/1.5 -apple-system,system-ui,sans-serif;color:${remote ? '' : ''}CanvasText;background:transparent;word-wrap:break-word}
      img{max-width:100%;height:auto} blockquote{border-left:3px solid #ccc;margin:8px 0;padding-left:10px;color:#666} pre{white-space:pre-wrap}
      </style></head><body>${out}</body></html>`;
    building = false;
  }

  function loadRemote(): void {
    const m = app.open;
    if (!m) return;
    showRemote = true;
    build(m.message.id, m.body.html, true);
  }

  async function save(part: string, filename: string): Promise<void> {
    try {
      const path = await emx.savePart(app.account, app.open!.message.id, part, filename);
      app.say(`Saved to ${path}`);
    } catch (e) {
      app.fail(e);
    }
  }
</script>

<article>
  {#if !app.open}
    <div class="empty muted">
      <p>Nothing open.</p>
      <p class="small"><kbd>j</kbd> <kbd>k</kbd> move, <kbd>e</kbd> archive, <kbd>#</kbd> trash, <kbd>r</kbd> reply, <kbd>c</kbd> new, <kbd>/</kbd> search</p>
    </div>
  {:else}
    {@const m = app.open.message}
    {@const b = app.open.body}
    <header>
      <h1>{m.subject || '(no subject)'}</h1>
      <div class="meta">
        <div>
          <strong>{m.from.name || m.from.address}</strong>
          {#if m.from.name}<span class="muted"> {m.from.address}</span>{/if}
          <div class="muted small">to {m.to.join(', ')}{#if m.cc.length}, cc {m.cc.join(', ')}{/if}</div>
        </div>
        <div class="muted small">{new Date(m.receivedAt).toLocaleString()}</div>
      </div>
      <div class="actions">
        <button class="btn sm" onclick={() => onreply(false)}>Reply</button>
        <button class="btn sm" onclick={() => onreply(true)}>Reply all</button>
        <button class="btn sm" onclick={() => app.moveTo(m, 'archive')}>Archive</button>
        <button class="btn sm" onclick={() => app.trash(m)}>{app.mailbox?.role === 'trash' ? 'Delete' : 'Trash'}</button>
        <button class="btn quiet sm" onclick={() => app.toggleFlag(m)}>{m.keywords.includes('$flagged') ? 'Unflag' : 'Flag'}</button>
        <button class="btn quiet sm" onclick={() => app.toggleSeen(m)}>{m.keywords.includes('$seen') ? 'Mark unread' : 'Mark read'}</button>
        {#if m.dmarc === 'fail'}<span class="warn small">DMARC failed: the sender may not be who it says</span>{/if}
      </div>
    </header>
    {#if b.sealed}
      <div class="note">This mailbox is sealed. The message is encrypted to its owner's key and opens in the web client.</div>
    {:else}
      {#if b.remoteImages > 0 && !showRemote}
        <div class="note">
          {b.remoteImages} remote image{b.remoteImages === 1 ? '' : 's'} held back.
          <button class="btn sm" onclick={loadRemote}>Load</button>
        </div>
      {/if}
      {#if b.html}
        <iframe title="Message" sandbox="" srcdoc={html} class:dim={building}></iframe>
      {:else}
        <pre class="text">{b.text}</pre>
      {/if}
      {#if b.attachments.length}
        <footer>
          {#each b.attachments as at (at.part)}
            <button class="att" onclick={() => save(at.part, at.filename)} title="Save to Downloads">
              <span class="name">{at.filename || at.part}</span>
              <span class="muted small">{size(at.size)}</span>
            </button>
          {/each}
        </footer>
      {/if}
    {/if}
  {/if}
</article>

<style>
  article {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--panel);
  }
  .empty {
    height: 100%;
    display: grid;
    place-items: center;
    text-align: center;
  }
  header {
    padding: 16px 20px 12px;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  h1 {
    margin: 0;
    font-size: 18px;
    user-select: text;
  }
  .meta {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    user-select: text;
  }
  .actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    align-items: center;
  }
  .warn {
    color: var(--warn);
  }
  .note {
    margin: 12px 20px 0;
    padding: 8px 12px;
    border-radius: var(--r);
    background: var(--well);
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 13px;
  }
  iframe {
    flex: 1;
    border: 0;
    width: 100%;
    padding: 12px 20px;
    background: transparent;
    color-scheme: light dark;
  }
  iframe.dim {
    opacity: 0.6;
  }
  .text {
    flex: 1;
    overflow: auto;
    margin: 0;
    padding: 16px 20px;
    white-space: pre-wrap;
    font: inherit;
    user-select: text;
  }
  footer {
    border-top: 1px solid var(--border);
    padding: 10px 20px;
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .att {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--well);
    cursor: pointer;
    max-width: 240px;
  }
  .att:hover {
    background: var(--hover);
  }
  .att .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 220px;
  }
</style>
