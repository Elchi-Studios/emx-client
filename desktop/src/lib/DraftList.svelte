<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '$lib/state.svelte';
  import { emx, type SavedDraft } from '$lib/emx';

  let drafts = $state<SavedDraft[] | null>(null);

  onMount(async () => {
    try {
      drafts = await emx.listDrafts();
    } catch (e) {
      app.fail(e);
      app.draftsOpen = false;
    }
  });

  function text(d: SavedDraft, k: string): string {
    const v = d.body[k];
    return typeof v === 'string' ? v : '';
  }

  function pick(d: SavedDraft): void {
    app.draftsOpen = false;
    app.composing = {
      to: text(d, 'to'),
      cc: text(d, 'cc'),
      subject: text(d, 'subject'),
      text: text(d, 'text'),
      inReplyTo: text(d, 'inReplyTo') || undefined,
      references: Array.isArray(d.body.references) ? (d.body.references as string[]) : undefined,
      draft: { id: d.id, account: d.account, version: d.version, from: text(d, 'from') }
    };
  }

  function key(e: KeyboardEvent): void {
    if (e.key === 'Escape') app.draftsOpen = false;
  }
</script>

<svelte:window onkeydown={key} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (app.draftsOpen = false)}>
  <div class="sheet">
    <h2>Saved drafts</h2>
    {#if drafts === null}
      <p class="muted">Loading</p>
    {:else if drafts.length === 0}
      <p class="muted">No saved drafts.</p>
    {:else}
      {#each drafts as d (d.id)}
        <button class="item" onclick={() => pick(d)}>
          <strong>{text(d, 'subject') || '(no subject)'}</strong>
          <span class="muted small">{text(d, 'to') || 'no recipient'}</span>
        </button>
      {/each}
    {/if}
    <button class="btn quiet" onclick={() => (app.draftsOpen = false)}>Close</button>
  </div>
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
    width: min(520px, 94vw);
    max-height: 80vh;
    overflow: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
    border: 0;
    border-radius: var(--r);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: var(--hover);
  }
</style>
