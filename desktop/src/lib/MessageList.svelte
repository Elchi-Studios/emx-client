<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { app, when } from '$lib/state.svelte';
  import { fieldsOf, isSealed } from '$lib/compose';
  import type { SavedDraft } from '$lib/emx';

  // Who wrote a draft and who changed it last, when that was somebody
  // else: a shared mailbox's drafts are written together.
  function draftBy(d: SavedDraft): string {
    const by = d.createdBy?.name ?? '';
    const edited = d.editedBy?.name ?? '';
    if (edited && edited !== by) return by ? `by ${by}, changed by ${edited}` : `changed by ${edited}`;
    return by ? `by ${by}` : '';
  }

  let q = $state('');
  let timer: ReturnType<typeof setTimeout>;
  let rows: HTMLDivElement;
  function typed(): void {
    clearTimeout(timer);
    timer = setTimeout(() => app.search(q), 250);
  }

  // Another folder or account: the box loses the words of the last
  // search, a search still waiting to start is dropped, and the list
  // starts at the top instead of where the previous one was scrolled.
  const folder = $derived(`${app.account}/${app.mailbox?.id ?? ''}`);
  $effect(() => {
    void folder;
    clearTimeout(timer);
    q = '';
    if (rows) rows.scrollTop = 0;
  });
  // A new search starts at the top too. A search that ends because the
  // folder was clicked again leaves the box empty as well.
  $effect(() => {
    if (!app.query) {
      clearTimeout(timer);
      q = '';
    }
    if (rows) rows.scrollTop = 0;
  });
  function scrolled(e: Event): void {
    const el = e.currentTarget as HTMLElement;
    if (el.scrollTop + el.clientHeight > el.scrollHeight - 200) app.more();
  }
</script>

<section>
  <header>
    <input id="search" class="field" placeholder="Search all folders" bind:value={q} oninput={typed} />
  </header>
  <div class="list" onscroll={scrolled} bind:this={rows}>
    {#if app.query}
      <p class="muted small pad">{app.searching ? 'Searching' : `${app.messages.length}${app.cursor ? '+' : ''} found for "${app.query}"`}</p>
    {:else if app.mailbox?.role === 'drafts' && app.drafts.length}
      <p class="muted small heading">Being written</p>
      {#each app.drafts as d (d.id)}
        {@const f = fieldsOf(d.body)}
        <button class="row" onclick={() => app.editDraft(d)}>
          <span class="dot"></span>
          <span class="body">
            <span class="line">
              <span class="who">{isSealed(d.body) ? 'Sealed draft' : f.to || '(no recipient)'}</span>
              <span class="time muted small">{when(d.updatedAt)}</span>
            </span>
            <span class="subject">{isSealed(d.body) ? 'Opens in the web client' : f.subject || '(no subject)'}</span>
            <span class="snippet muted small">{draftBy(d)}</span>
          </span>
        </button>
      {/each}
      <p class="muted small heading">In the folder</p>
    {/if}
    {#each app.messages as m (m.id)}
      {@const unseen = !m.keywords.includes('$seen')}
      <button class="row" class:unseen class:on={(app.selected ?? app.open?.message.id) === m.id} onclick={() => app.read(m)}>
        <span class="dot" class:flag={m.keywords.includes('$flagged')}></span>
        <span class="body">
          <span class="line">
            <span class="who">{m.from.name || m.from.address}</span>
            <span class="time muted small">{when(m.receivedAt)}</span>
          </span>
          <span class="subject">{m.subject || '(no subject)'}{#if m.hasAttachments}<span class="clip" title="attachment">⊕</span>{/if}</span>
          <span class="snippet muted small">{m.sealed ? 'Sealed' : m.snippet}</span>
        </span>
      </button>
    {/each}
    {#if app.loading}
      <p class="muted small pad">Loading</p>
    {:else if app.messages.length === 0 && !app.query}
      <p class="muted pad">Nothing here.</p>
    {/if}
  </div>
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: var(--panel);
    min-height: 0;
  }
  header {
    padding: 10px;
    border-bottom: 1px solid var(--border);
  }
  .list {
    overflow: auto;
    flex: 1;
  }
  .pad {
    padding: 12px;
    margin: 0;
  }
  .heading {
    margin: 0;
    padding: 8px 12px 4px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .row {
    display: flex;
    gap: 8px;
    width: 100%;
    padding: 9px 12px 9px 8px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.on {
    background: var(--accent-soft);
  }
  .dot {
    width: 7px;
    height: 7px;
    margin-top: 7px;
    border-radius: 50%;
    flex: none;
    background: transparent;
  }
  .row.unseen .dot {
    background: var(--accent);
  }
  .dot.flag {
    background: var(--warn) !important;
    border-radius: 1px;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    flex: 1;
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
  .who {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row.unseen .who,
  .row.unseen .subject {
    font-weight: 600;
  }
  .subject,
  .snippet {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .clip {
    margin-left: 6px;
    color: var(--muted);
    font-size: 11px;
  }
</style>
