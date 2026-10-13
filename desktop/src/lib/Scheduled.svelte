<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { app } from '$lib/state.svelte';
  import { fieldsOf, isSealed, localInput, pickedTime } from '$lib/compose';
  import type { Held } from '$lib/emx';

  // The held message whose time is being changed, and the time picked.
  let moving = $state<string | null>(null);
  let picked = $state('');

  function startMove(h: Held): void {
    moving = h.id;
    picked = localInput(new Date(h.sendAt));
  }

  async function move(h: Held): Promise<void> {
    const at = pickedTime(picked, new Date());
    if (!at) {
      app.fail('Pick a time ahead, within a year.');
      return;
    }
    moving = null;
    await app.reschedule(h, at);
  }

  async function takeBack(h: Held): Promise<void> {
    const failed = h.state === 'failed';
    const ok = await app.ask(
      failed
        ? 'Open this message as a draft again, to change it and send it anew?'
        : 'Take this message back? It is not sent, and opens as a draft again.',
      failed ? 'Open as draft' : 'Take back'
    );
    if (ok) await app.takeBack(h);
  }
</script>

<section>
  <header>
    <strong>Scheduled</strong>
    <span class="muted small">{app.accountName}</span>
    <span class="grow"></span>
    <button class="btn quiet sm" onclick={() => app.loadHeld()}>Refresh</button>
  </header>
  <div class="list">
    {#each app.held as h (h.id)}
      {@const f = fieldsOf(h.message?.body)}
      {@const sealed = isSealed(h.message?.body)}
      <div class="row" class:failed={h.state === 'failed'}>
        <div class="line">
          <span class="who">{sealed ? 'Sealed' : f.to || '(no recipient)'}</span>
          <span class="muted small">{new Date(h.sendAt).toLocaleString(undefined, { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })}</span>
        </div>
        <div class="subject">{sealed ? 'Opens in the web client' : f.subject || '(no subject)'}</div>
        {#if h.state === 'failed'}
          <div class="warn small">Not sent: {h.errorMessage || h.errorCode}</div>
        {:else if h.kind === 'undo'}
          <div class="muted small">About to go</div>
        {:else if h.sender?.name}
          <div class="muted small">Held by {h.sender.name}</div>
        {/if}
        <div class="actions">
          {#if moving === h.id}
            <input type="datetime-local" class="field" bind:value={picked} min={localInput(new Date())} />
            <button class="btn sm primary" onclick={() => move(h)}>Save time</button>
            <button class="btn quiet sm" onclick={() => (moving = null)}>Cancel</button>
          {:else}
            {#if h.state !== 'failed' && h.kind !== 'undo'}<button class="btn sm" onclick={() => startMove(h)}>Change time</button>{/if}
            <button class="btn sm" onclick={() => takeBack(h)}>{h.state === 'failed' ? 'Open as draft' : 'Take back'}</button>
          {/if}
        </div>
      </div>
    {:else}
      <p class="muted pad">{app.heldLoading ? 'Loading' : 'Nothing scheduled. Send later in the composer holds a message here until its time.'}</p>
    {/each}
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
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px;
    border-bottom: 1px solid var(--border);
  }
  .grow {
    flex: 1;
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
    flex-direction: column;
    gap: 2px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }
  .row.failed {
    border-left: 3px solid var(--warn);
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
  .who,
  .subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .warn {
    color: var(--warn);
  }
  .actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 6px;
    align-items: center;
  }
  .actions .field {
    width: auto;
  }
</style>
