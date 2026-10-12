<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<script lang="ts">
  import { app } from '$lib/state.svelte';
  import { quotable, replyRecipients } from '$lib/mail';
  import Sidebar from './Sidebar.svelte';
  import MessageList from './MessageList.svelte';
  import MessageView from './MessageView.svelte';
  import Composer from './Composer.svelte';

  // Keys, when nothing is being typed or chosen, and no composer or
  // question is open over the list: a key meant for those must not act
  // on the message behind them.
  function keys(e: KeyboardEvent): void {
    if (app.composing || app.asking) return;
    const t = e.target as HTMLElement;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)) return;
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
        // From the message last asked for, which may still be loading.
        const at = app.selected ?? cur?.id;
        const i = app.messages.findIndex((m) => m.id === at);
        const next = app.messages[e.key === 'j' ? i + 1 : i - 1];
        if (next) app.read(next);
        break;
      }
      case 'e':
        if (cur) app.moveTo(cur, 'archive');
        break;
      // Not Backspace: it is pressed by reflex, and in Trash this deletes.
      case '#':
      case 'Delete':
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
        app.close();
        break;
    }
  }

  function reply(all: boolean): void {
    const m = app.open;
    if (!m) return;
    // Every address the person sends from is theirs: Reply all leaves
    // them out, and a reply to their own message goes to its recipients.
    const own = [app.accountName, ...(app.me?.sendFrom ?? []).map((s) => s.address)].filter(Boolean);
    const recipients = replyRecipients(
      {
        from: m.message.from.address,
        to: m.message.to,
        cc: m.message.cc,
        replyTo: m.body.replyTo.map((a) => a.address)
      },
      own,
      all
    );
    // An HTML-only message is quoted from a text rendering of its HTML.
    const quoted = quotable(m.body)
      .split('\n')
      .map((l) => '> ' + l)
      .join('\n');
    app.composing = {
      to: recipients.join(', '),
      subject: m.message.subject.match(/^re:/i) ? m.message.subject : 'Re: ' + m.message.subject,
      text: `\n\nOn ${new Date(m.message.receivedAt).toLocaleString()}, ${m.message.from.name || m.message.from.address} wrote:\n${quoted}`,
      inReplyTo: m.body.messageId,
      references: [...m.body.references, m.body.messageId].filter(Boolean)
    };
  }

  async function signOut(): Promise<void> {
    await app.signOut();
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
