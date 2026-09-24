<script lang="ts">
  import { app } from '$lib/state.svelte';
  import { emx } from '$lib/emx';
  import Sidebar from './Sidebar.svelte';
  import MessageList from './MessageList.svelte';
  import MessageView from './MessageView.svelte';
  import Composer from './Composer.svelte';

  // Keys, when nothing is being typed.
  function keys(e: KeyboardEvent): void {
    const t = e.target as HTMLElement;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const cur = app.open?.message;
    switch (e.key) {
      case 'c':
        app.composing = {};
        break;
      case '/':
        e.preventDefault();
        (document.querySelector('#search') as HTMLInputElement | null)?.focus();
        break;
      case 'j':
      case 'k': {
        const i = app.messages.findIndex((m) => m.id === cur?.id);
        const next = app.messages[e.key === 'j' ? i + 1 : i - 1];
        if (next) app.read(next);
        break;
      }
      case 'e':
        if (cur) app.moveTo(cur, 'archive');
        break;
      case '#':
      case 'Delete':
      case 'Backspace':
        if (cur) app.trash(cur);
        break;
      case 'u':
        if (cur) app.toggleSeen(cur);
        break;
      case 's':
        if (cur) app.toggleFlag(cur);
        break;
      case 'r':
        if (cur) reply(false);
        break;
      case 'a':
        if (cur) reply(true);
        break;
      case 'Escape':
        app.open = null;
        break;
    }
  }

  function reply(all: boolean): void {
    const m = app.open;
    if (!m) return;
    const me = app.accountName.toLowerCase();
    const to = m.body.replyTo.length ? m.body.replyTo.map((a) => a.address) : [m.message.from.address];
    const others = all ? [...m.message.to, ...m.message.cc].filter((a) => !a.toLowerCase().includes(me)) : [];
    const quoted = (m.body.text || '')
      .split('\n')
      .map((l) => '> ' + l)
      .join('\n');
    app.composing = {
      to: [...to, ...others].join(', '),
      subject: m.message.subject.match(/^re:/i) ? m.message.subject : 'Re: ' + m.message.subject,
      text: `\n\nOn ${new Date(m.message.receivedAt).toLocaleString()}, ${m.message.from.name || m.message.from.address} wrote:\n${quoted}`,
      inReplyTo: m.body.messageId,
      references: [...m.body.references, m.body.messageId].filter(Boolean)
    };
  }

  async function signOut(): Promise<void> {
    await emx.signOut();
    app.me = null;
    app.open = null;
    app.messages = [];
  }
</script>

<svelte:window onkeydown={keys} />

<div class="shell">
  <Sidebar onsignout={signOut} />
  <MessageList />
  <MessageView onreply={reply} />
</div>

{#if app.composing}
  <Composer />
{/if}

<style>
  .shell {
    display: grid;
    grid-template-columns: 220px minmax(300px, 380px) 1fr;
    height: 100vh;
  }
</style>
