<script lang="ts">
  import { app } from '$lib/state.svelte';
  import { emx, describe } from '$lib/emx';

  let token = $state('');
  let baseUrl = $state('');
  let advanced = $state(false);
  let busy = $state(false);
  let error = $state('');

  async function go(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      const me = await emx.signIn(baseUrl, token);
      await app.start(me);
    } catch (err) {
      error = describe(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="wrap">
  <form class="card" onsubmit={go}>
    <div class="mark">EMX</div>
    <h1>Sign in with a token</h1>
    <p class="muted">
      Make one in the EMX web client under Settings, Developer API, with the <span class="mono">mail:write</span> scope.
      It stays on this computer, readable by you alone.
    </p>
    <label>
      <span class="muted small">Token</span>
      <input class="field mono" bind:value={token} placeholder="emx_..." autocomplete="off" spellcheck="false" required />
    </label>
    {#if advanced}
      <label>
        <span class="muted small">Server</span>
        <input class="field" bind:value={baseUrl} placeholder="https://mail.emxmail.app" />
      </label>
    {:else}
      <button type="button" class="btn quiet sm" onclick={() => (advanced = true)}>Another server</button>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <button class="btn primary" disabled={busy || !token}>{busy ? 'Signing in' : 'Sign in'}</button>
    <p class="muted small">Made by Elchi Studios, Zug. <a href="https://emxmail.ch" target="_blank" rel="noopener">emxmail.ch</a></p>
    <p class="muted small">Free software under the GNU Affero General Public License 3.0, with no warranty. The source is at <span class="mono">github.com/Elchi-Studios/emx-client</span>.</p>
  </form>
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
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .error {
    color: var(--danger);
  }
</style>
