<script lang="ts">
  import { app, when } from '$lib/state.svelte';

  let q = $state('');
  let timer: ReturnType<typeof setTimeout>;
  function typed(): void {
    clearTimeout(timer);
    timer = setTimeout(() => app.search(q), 250);
  }
  function scrolled(e: Event): void {
    const el = e.currentTarget as HTMLElement;
    if (el.scrollTop + el.clientHeight > el.scrollHeight - 200) app.more();
  }
</script>

<section>
  <header>
    <input id="search" class="field" placeholder="Search {app.mailbox?.name ?? ''}" bind:value={q} oninput={typed} />
  </header>
  <div class="list" onscroll={scrolled}>
    {#if app.query}
      <p class="muted small pad">{app.searching ? 'Searching' : `${app.messages.length} found for "${app.query}"`}</p>
    {/if}
    {#each app.messages as m (m.id)}
      {@const unseen = !m.keywords.includes('$seen')}
      <button class="row" class:unseen class:on={app.open?.message.id === m.id} onclick={() => app.read(m)}>
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
