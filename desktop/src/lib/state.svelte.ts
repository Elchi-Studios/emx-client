// What the window shows, and how it stays current: the lists are loaded
// once per folder and then kept up to date from the change feed, the way
// the web client does it.

import { emx, describe, type Me, type Mailbox, type Message, type FullMessage } from './emx';

export const roleOrder = ['inbox', 'drafts', 'sent', 'archive', 'junk', 'trash'];

class State {
  me = $state<Me | null>(null);
  account = $state('me');
  mailboxes = $state<Mailbox[]>([]);
  mailbox = $state<Mailbox | null>(null);
  messages = $state<Message[]>([]);
  cursor = $state('');
  loading = $state(false);
  open = $state<FullMessage | null>(null);
  query = $state('');
  searching = $state(false);
  composing = $state<null | { to?: string; subject?: string; text?: string; inReplyTo?: string; references?: string[] }>(null);
  notice = $state('');
  problem = $state('');
  modseq: Record<string, number> = {};
  private busy: Record<string, boolean> = {};

  get accounts() {
    return this.me?.accounts ?? [];
  }

  get accountName(): string {
    const a = this.accounts.find((x) => x.id === this.account || (this.account === 'me' && x.rights.own));
    return a?.address ?? '';
  }

  say(text: string): void {
    this.notice = text;
    setTimeout(() => (this.notice = ''), 4000);
  }

  fail(e: unknown): void {
    this.problem = describe(e);
    setTimeout(() => (this.problem = ''), 6000);
  }

  async start(me: Me): Promise<void> {
    this.me = me;
    const own = me.accounts.find((a) => a.rights.own);
    this.account = own?.id ?? 'me';
    await this.loadMailboxes();
    await emx.onChange((n) => this.onChange(n.account, n.modseq));
  }

  async switchAccount(id: string): Promise<void> {
    this.account = id;
    this.open = null;
    this.query = '';
    await this.loadMailboxes();
  }

  async loadMailboxes(): Promise<void> {
    try {
      const list = await emx.mailboxes(this.account);
      list.sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
      this.mailboxes = list;
      this.modseq[this.account] = Math.max(0, ...list.map((m) => m.modseq));
      const keep = this.mailbox && list.find((m) => m.id === this.mailbox!.id);
      await this.openMailbox(keep ?? list.find((m) => m.role === 'inbox') ?? list[0]);
    } catch (e) {
      this.fail(e);
    }
  }

  async openMailbox(m: Mailbox | undefined): Promise<void> {
    if (!m) return;
    this.mailbox = m;
    this.query = '';
    this.messages = [];
    this.cursor = '';
    this.loading = true;
    try {
      const page = await emx.messages(this.account, m.id);
      this.messages = page.messages;
      this.cursor = page.cursor;
    } catch (e) {
      this.fail(e);
    } finally {
      this.loading = false;
    }
  }

  async more(): Promise<void> {
    if (!this.cursor || this.loading || !this.mailbox) return;
    this.loading = true;
    try {
      const page = await emx.messages(this.account, this.mailbox.id, this.cursor);
      this.messages = [...this.messages, ...page.messages];
      this.cursor = page.cursor;
    } catch (e) {
      this.fail(e);
    } finally {
      this.loading = false;
    }
  }

  async search(q: string): Promise<void> {
    this.query = q;
    if (!q.trim()) {
      await this.openMailbox(this.mailbox ?? undefined);
      return;
    }
    this.searching = true;
    try {
      this.messages = await emx.search(this.account, q);
      this.cursor = '';
    } catch (e) {
      this.fail(e);
    } finally {
      this.searching = false;
    }
  }

  async read(m: Message): Promise<void> {
    try {
      this.open = await emx.message(this.account, m.id);
      if (!m.keywords.includes('$seen')) {
        await emx.keywords(this.account, [m.id], ['$seen'], []);
        this.patch(m.id, (x) => ({ ...x, keywords: [...x.keywords, '$seen'] }));
        this.count(m.mailboxId, -1);
      }
    } catch (e) {
      this.fail(e);
    }
  }

  async toggleSeen(m: Message): Promise<void> {
    const seen = m.keywords.includes('$seen');
    try {
      await emx.keywords(this.account, [m.id], seen ? [] : ['$seen'], seen ? ['$seen'] : []);
      this.patch(m.id, (x) => ({ ...x, keywords: seen ? x.keywords.filter((k) => k !== '$seen') : [...x.keywords, '$seen'] }));
      this.count(m.mailboxId, seen ? 1 : -1);
    } catch (e) {
      this.fail(e);
    }
  }

  async toggleFlag(m: Message): Promise<void> {
    const on = m.keywords.includes('$flagged');
    try {
      await emx.keywords(this.account, [m.id], on ? [] : ['$flagged'], on ? ['$flagged'] : []);
      this.patch(m.id, (x) => ({ ...x, keywords: on ? x.keywords.filter((k) => k !== '$flagged') : [...x.keywords, '$flagged'] }));
    } catch (e) {
      this.fail(e);
    }
  }

  async moveTo(m: Message, role: string): Promise<void> {
    const target = this.mailboxes.find((b) => b.role === role);
    if (!target) return;
    try {
      await emx.move(this.account, [m.id], target.id);
      this.remove(m.id);
      this.say(`Moved to ${target.name}`);
    } catch (e) {
      this.fail(e);
    }
  }

  async trash(m: Message): Promise<void> {
    if (this.mailbox?.role === 'trash') {
      try {
        await emx.delete(this.account, [m.id]);
        this.remove(m.id);
        this.say('Deleted');
      } catch (e) {
        this.fail(e);
      }
      return;
    }
    await this.moveTo(m, 'trash');
  }

  /// A change notice: ask what changed since the last modseq handled and
  /// fold it into the lists. Notices during a fetch are coalesced.
  async onChange(account: string, _modseq: number): Promise<void> {
    if (account !== this.account) return;
    if (this.busy[account]) return;
    this.busy[account] = true;
    try {
      let since = this.modseq[account] ?? 0;
      for (let i = 0; i < 20; i++) {
        let ch;
        try {
          ch = await emx.changes(account, since);
        } catch (e) {
          if ((e as { code?: string }).code === 'reload') {
            await this.loadMailboxes();
            return;
          }
          throw e;
        }
        since = ch.modseq;
        this.modseq[account] = since;
        this.apply(ch.updated, ch.destroyed);
        if (!ch.hasMore) break;
      }
      // Counts come from the server; a change may have touched any folder.
      const list = await emx.mailboxes(account);
      list.sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
      this.mailboxes = list;
      if (this.mailbox) this.mailbox = list.find((m) => m.id === this.mailbox!.id) ?? this.mailbox;
    } catch (e) {
      this.fail(e);
    } finally {
      this.busy[account] = false;
    }
  }

  private apply(updated: Message[], destroyed: string[]): void {
    if (this.query) return;
    let list = this.messages.filter((m) => !destroyed.includes(m.id));
    for (const u of updated) {
      const i = list.findIndex((m) => m.id === u.id);
      const here = u.mailboxId === this.mailbox?.id;
      if (i >= 0 && here) list[i] = u;
      else if (i >= 0) list.splice(i, 1);
      else if (here) list = [u, ...list];
    }
    list.sort((a, b) => (a.receivedAt < b.receivedAt ? 1 : a.receivedAt > b.receivedAt ? -1 : 0));
    this.messages = list;
    if (this.open && destroyed.includes(this.open.message.id)) this.open = null;
  }

  private patch(id: string, fn: (m: Message) => Message): void {
    this.messages = this.messages.map((m) => (m.id === id ? fn(m) : m));
    if (this.open?.message.id === id) this.open = { ...this.open, message: fn(this.open.message) };
  }

  private remove(id: string): void {
    this.messages = this.messages.filter((m) => m.id !== id);
    if (this.open?.message.id === id) this.open = null;
  }

  private count(mailboxId: string, delta: number): void {
    this.mailboxes = this.mailboxes.map((b) => (b.id === mailboxId ? { ...b, unseen: Math.max(0, b.unseen + delta) } : b));
    if (this.mailbox?.id === mailboxId) this.mailbox = { ...this.mailbox, unseen: Math.max(0, this.mailbox.unseen + delta) };
  }
}

function rank(m: Mailbox): number {
  const i = roleOrder.indexOf(m.role);
  return i < 0 ? 100 + m.sortOrder : i;
}

export const app = new State();

export function when(s: string | null): string {
  if (!s) return '';
  const d = new Date(s);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  if (d.getFullYear() === now.getFullYear()) return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
  return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
}

export function size(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}
