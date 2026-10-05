<script lang="ts">
  let {
    message,
    trying,
    onretry,
    onsignout
  }: { message: string; trying: boolean; onretry: () => void; onsignout: () => void } = $props();
</script>

<div class="wrap">
  <div class="card" role="status">
    <div class="mark">EMX</div>
    <h1>No connection</h1>
    <p>{message}</p>
    <p class="muted">You are still signed in. The app connects on its own as soon as EMX can be reached again.</p>
    <div class="foot">
      <button class="btn primary" onclick={onretry} disabled={trying}>{trying ? 'Connecting' : 'Try now'}</button>
      <span class="grow"></span>
      <button class="btn quiet sm" onclick={onsignout}>Sign out</button>
    </div>
  </div>
</div>

<style>
  .wrap {
    height: 100vh;
    display: grid;
    place-items: center;
    padding: 24px;
  }
  .card {
    width: min(440px, 100%);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 28px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .mark {
    width: 44px;
    height: 44px;
    border-radius: 12px;
    background: var(--accent);
    color: #fff;
    font-weight: 800;
    font-size: 13px;
    display: grid;
    place-items: center;
  }
  h1 {
    font-size: 20px;
    margin: 0;
  }
  p {
    margin: 0;
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
