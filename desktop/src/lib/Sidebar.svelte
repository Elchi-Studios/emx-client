<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { app } from '$lib/state.svelte';

  let { onsignout }: { onsignout: () => void } = $props();
  const icons: Record<string, string> = { inbox: '▣', drafts: '✎', sent: '➤', archive: '▤', junk: '⊘', trash: '🗑', '': '▫' };
</script>

<aside>
  <div class="top">
    <button class="btn primary" onclick={() => (app.composing = {})}>New message <kbd>c</kbd></button>
  </div>
  {#if app.accounts.length > 1}
    <select class="field small" value={app.account} onchange={(e) => app.switchAccount(e.currentTarget.value)}>
      {#each app.accounts as a (a.id)}<option value={a.id}>{a.address}</option>{/each}
    </select>
  {/if}
  <nav>
    {#each app.mailboxes as m (m.id)}
      <button class="box" class:on={app.view === 'mail' && app.mailbox?.id === m.id && !app.query} onclick={() => app.openMailbox(m)}>
        <span class="icon">{icons[m.role] ?? icons['']}</span>
        <span class="name">{m.name}</span>
        {#if m.unseen > 0}<span class="count">{m.unseen}</span>{/if}
      </button>
    {/each}
    {#if app.mayDraft}
      <button class="box" class:on={app.view === 'scheduled'} onclick={() => app.showScheduled()} title="Messages held to go later, and those that could not go">
        <span class="icon">◷</span>
        <span class="name">Scheduled</span>
        {#if app.view === 'scheduled' && app.held.some((h) => h.state === 'failed')}<span class="count warn">!</span>{/if}
      </button>
    {/if}
  </nav>
  <div class="grow"></div>
  <div class="me">
    <div class="who">
      <strong>{app.me?.name}</strong>
      <span class="muted small">{app.accountName}</span>
    </div>
    <button class="btn quiet sm" onclick={onsignout}>Sign out</button>
  </div>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: var(--well);
    padding: 12px 10px;
    gap: 10px;
    overflow: auto;
  }
  .top .btn {
    width: 100%;
    justify-content: space-between;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .box {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--r);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .box:hover {
    background: var(--hover);
  }
  .box.on {
    background: var(--panel);
    font-weight: 600;
  }
  .icon {
    width: 18px;
    text-align: center;
    color: var(--muted);
    font-size: 12px;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 11px;
    font-weight: 700;
    padding: 1px 6px;
    border-radius: 9px;
    background: var(--accent-soft);
    color: var(--accent);
  }
  .count.warn {
    background: var(--warn);
    color: #fff;
  }
  .grow {
    flex: 1;
  }
  .me {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }
  .who {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .who span,
  .who strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
