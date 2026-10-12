// What the window shows, and how it stays current: the lists are loaded
// once per folder and then kept up to date from the change feed, the way
// the web client does it.

import { emx, describe, type Me, type Mailbox, type Message, type FullMessage } from './emx';
import { appendPage, applyToFolder, applyToResults, deletesForGood, Latest, unique, withKeyword } from './list';

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
  // The message last asked for, set at once, before it has loaded: j and
  // k move on from here, so two quick presses open two messages.
  selected = $state<string | null>(null);
  query = $state('');
  searching = $state(false);
  composing = $state<null | { to?: string; subject?: string; text?: string; inReplyTo?: string; references?: string[] }>(null);
  notice = $state('');
  problem = $state('');
  // A question waiting for an answer, such as "Delete for good?". While
  // one is open, the keys of the window and the composer do nothing.
  asking = $state<null | { text: string; confirm: string; answer: (ok: boolean) => void }>(null);
  modseq: Record<string, number> = {};
  private busy: Record<string, boolean> = {};
  // A notice that came while that account's changes were being fetched:
  // they are fetched once more when the fetch ends.
  private again: Record<string, boolean> = {};
  // What the list shows (a folder, a search) and which message is open:
  // only the answer to the newest request is applied.
  private view = new Latest();
  private opening = new Latest();

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

  /// Asks before something that cannot be undone. Resolves true when the
  /// person confirms; another question replacing this one answers no.
  ask(text: string, confirm: string): Promise<boolean> {
    this.asking?.answer(false);
    return new Promise((resolve) => {
      const answer = (ok: boolean): void => {
        if (this.asking?.answer === answer) this.asking = null;
        resolve(ok);
      };
      this.asking = { text, confirm, answer };
    });
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
    this.selected = null;
    this.opening.next();
    this.query = '';
    await this.loadMailboxes();
  }

  async loadMailboxes(): Promise<void> {
    const account = this.account;
    try {
      const list = await emx.mailboxes(account);
      if (account !== this.account) return;
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
    const n = this.view.next();
    this.mailbox = m;
    this.query = '';
    this.searching = false;
    this.messages = [];
    this.cursor = '';
    this.loading = true;
    try {
      const page = await emx.messages(this.account, m.id);
      if (!this.view.is(n)) return;
      this.messages = unique(page.messages);
      this.cursor = page.cursor;
    } catch (e) {
      if (this.view.is(n)) this.fail(e);
    } finally {
      if (this.view.is(n)) this.loading = false;
    }
  }

  /// The next page of what the list shows. It belongs to the view it was
  /// asked for; when the folder changes meanwhile, it is dropped.
  async more(): Promise<void> {
    if (!this.cursor || this.loading || this.searching || !this.mailbox || this.query) return;
    const n = this.view.current;
    this.loading = true;
    try {
      const page = await emx.messages(this.account, this.mailbox.id, this.cursor);
      if (!this.view.is(n)) return;
      this.messages = appendPage(this.messages, page.messages);
      this.cursor = page.cursor;
    } catch (e) {
      if (this.view.is(n)) this.fail(e);
    } finally {
      if (this.view.is(n)) this.loading = false;
    }
  }

  async search(q: string): Promise<void> {
    if (!q.trim()) {
      await this.openMailbox(this.mailbox ?? undefined);
      return;
    }
    const n = this.view.next();
    this.query = q;
    this.searching = true;
    this.loading = false;
    try {
      const found = await emx.search(this.account, q);
      if (!this.view.is(n)) return;
      this.messages = unique(found);
      this.cursor = '';
    } catch (e) {
      if (this.view.is(n)) this.fail(e);
    } finally {
      if (this.view.is(n)) this.searching = false;
    }
  }

  async read(m: Message): Promise<void> {
    const n = this.opening.next();
    this.selected = m.id;
    try {
      const full = await emx.message(this.account, m.id);
      if (!this.opening.is(n)) return;
      this.open = full;
      if (!full.message.keywords.includes('$seen')) await this.setSeen(full.message, true);
    } catch (e) {
      if (this.opening.is(n)) this.fail(e);
    }
  }

  /// Closes the open message, and makes one still loading come to
  /// nothing.
  close(): void {
    this.opening.next();
    this.open = null;
    this.selected = null;
  }

  async toggleSeen(m: Message): Promise<void> {
    try {
      await this.setSeen(m, !m.keywords.includes('$seen'));
    } catch (e) {
      this.fail(e);
    }
  }

  /// Marks a message read or unread. The folder's unread count moves only
  /// when the service says the message changed: one already read on the
  /// phone is not counted twice.
  private async setSeen(m: Message, seen: boolean): Promise<void> {
    const changed = await emx.keywords(this.account, [m.id], seen ? ['$seen'] : [], seen ? [] : ['$seen']);
    this.patch(m.id, (x) => ({ ...x, keywords: withKeyword(x.keywords, '$seen', seen) }));
    if (changed.includes(m.id)) this.count(m.mailboxId, seen ? -1 : 1);
  }

  async toggleFlag(m: Message): Promise<void> {
    const on = !m.keywords.includes('$flagged');
    try {
      await emx.keywords(this.account, [m.id], on ? ['$flagged'] : [], on ? [] : ['$flagged']);
      this.patch(m.id, (x) => ({ ...x, keywords: withKeyword(x.keywords, '$flagged', on) }));
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

  /// Whether Delete removes `m` for good rather than moving it to Trash.
  forGood(m: Message): boolean {
    return deletesForGood(m, this.mailboxes);
  }

  async trash(m: Message): Promise<void> {
    if (this.forGood(m)) {
      // In Trash or Junk this is the second step and cannot be undone.
      if (!(await this.ask('Delete this message for good? It cannot be brought back.', 'Delete'))) return;
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
  /// fold it into the lists. A notice that comes during a fetch is not
  /// lost: the fetch runs once more when it ends, for everything after
  /// the modseq it reached.
  async onChange(account: string, _modseq: number): Promise<void> {
    if (account !== this.account) return;
    if (this.busy[account]) {
      this.again[account] = true;
      return;
    }
    this.busy[account] = true;
    try {
      do {
        this.again[account] = false;
        await this.catchUp(account);
      } while (this.again[account] && account === this.account);
    } finally {
      this.busy[account] = false;
      this.again[account] = false;
    }
  }

  private async catchUp(account: string): Promise<void> {
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
        if (account !== this.account) return;
        since = ch.modseq;
        this.modseq[account] = since;
        this.apply(ch.updated, ch.destroyed);
        if (!ch.hasMore) break;
      }
      // Counts come from the server; a change may have touched any folder.
      const list = await emx.mailboxes(account);
      if (account !== this.account) return;
      list.sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
      this.mailboxes = list;
      if (this.mailbox) this.mailbox = list.find((m) => m.id === this.mailbox!.id) ?? this.mailbox;
    } catch (e) {
      this.fail(e);
    }
  }

  private apply(updated: Message[], destroyed: string[]): void {
    // While a folder is still loading, its first page brings the current
    // state; a list being replaced is left alone.
    if (this.query) this.messages = applyToResults(this.messages, updated, destroyed);
    else if (this.mailbox && !(this.loading && this.messages.length === 0))
      this.messages = applyToFolder(this.messages, updated, destroyed, this.mailbox.id, this.cursor !== '');
    if (!this.open) return;
    const id = this.open.message.id;
    if (destroyed.includes(id)) {
      this.close();
      return;
    }
    // The open message follows what other devices do to it, its flag
    // among them.
    const u = updated.find((m) => m.id === id);
    if (u) this.open = { ...this.open, message: u };
  }

  private patch(id: string, fn: (m: Message) => Message): void {
    this.messages = this.messages.map((m) => (m.id === id ? fn(m) : m));
    if (this.open?.message.id === id) this.open = { ...this.open, message: fn(this.open.message) };
  }

  private remove(id: string): void {
    this.messages = this.messages.filter((m) => m.id !== id);
    if (this.open?.message.id === id) this.close();
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
