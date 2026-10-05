<script lang="ts">
  import { app } from '$lib/state.svelte';

  // Cancel has the focus, so Enter or Space keeps what is there; Escape
  // answers no as well, and stops here so the window's own keys and the
  // composer's do not see it.
  let cancel: HTMLButtonElement;
  $effect(() => cancel?.focus());

  function key(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      app.asking?.answer(false);
    }
  }
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && app.asking?.answer(false)}>
  <div class="box" role="alertdialog" aria-modal="true" aria-labelledby="ask-text" tabindex="-1" onkeydown={key}>
    <p id="ask-text">{app.asking?.text}</p>
    <div class="foot">
      <button class="btn" bind:this={cancel} onclick={() => app.asking?.answer(false)}>Cancel</button>
      <button class="btn danger" onclick={() => app.asking?.answer(true)}>{app.asking?.confirm}</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: grid;
    place-items: center;
    z-index: 60;
  }
  .box {
    width: min(420px, 92vw);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
  }
  p {
    margin: 0;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
  }
  .danger {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
</style>
