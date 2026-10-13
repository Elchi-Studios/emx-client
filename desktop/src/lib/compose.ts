// SPDX-License-Identifier: AGPL-3.0-only
//
// What the composer works out besides the text: the body a draft is
// saved as, the key a send goes with, the times Send later offers and
// what a snippet becomes when it is inserted. Without Svelte, so that it
// can be tested on its own.

/// The fields of a draft the desktop composer writes. A draft saved in
/// the web client has more (what it answers, files it forwards, a
/// reminder): those are kept as they are when the desktop saves it.
export interface DraftFields {
  from: string;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  text: string;
  inReplyTo?: string;
  references?: string[];
}

const str = (v: unknown): string => (typeof v === 'string' ? v : '');

/// The fields of a draft's body; empty where it has none.
export function fieldsOf(body: Record<string, unknown> | null | undefined): DraftFields {
  const b = body ?? {};
  const refs = Array.isArray(b.references) ? b.references.filter((r): r is string => typeof r === 'string') : [];
  return {
    from: str(b.from),
    to: str(b.to),
    cc: str(b.cc),
    bcc: str(b.bcc),
    subject: str(b.subject),
    text: str(b.text),
    inReplyTo: str(b.inReplyTo) || undefined,
    references: refs.length ? refs : undefined
  };
}

/// Whether a body is sealed to its mailbox's key, which only the web
/// client opens.
export function isSealed(body: Record<string, unknown> | null | undefined): boolean {
  return typeof body?.sealed === 'string';
}

/// The body to save: `base`, the body as it was loaded, with the fields
/// written here put in, and every other field left as it was.
export function bodyFor(base: Record<string, unknown> | null | undefined, f: DraftFields): Record<string, unknown> {
  const out: Record<string, unknown> = { ...(base ?? {}) };
  out.from = f.from;
  out.to = f.to;
  out.cc = f.cc;
  out.bcc = f.bcc;
  out.subject = f.subject;
  out.text = f.text;
  if (f.inReplyTo) out.inReplyTo = f.inReplyTo;
  if (f.references?.length) out.references = f.references;
  return out;
}

/// Whether two sets of fields say the same.
export function sameFields(a: DraftFields, b: DraftFields): boolean {
  return (
    a.from === b.from &&
    a.to === b.to &&
    a.cc === b.cc &&
    a.bcc === b.bcc &&
    a.subject === b.subject &&
    a.text === b.text
  );
}

function randomKey(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return 'emx-desktop-' + Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/// The idempotency key of a send. EMX answers a request that comes again
/// with its key with the first answer instead of sending twice, and
/// refuses the key with another request (`idempotency_mismatch`). So
/// the key stays while the request stays the same, pressing Send again
/// after a timeout among them, and a request that changed in any way,
/// a word, a recipient, the time to send at, gets a new one.
export class SendKey {
  private request = '';
  private key = '';

  /// The key for the request `request`, any text that says all of it.
  for(request: string): string {
    if (request !== this.request || !this.key) {
      this.request = request;
      this.key = randomKey();
    }
    return this.key;
  }

  /// Forgets the key, after an answer that said it belongs to another
  /// request: the next send gets a new one.
  forget(): void {
    this.key = '';
  }
}

/// How long a message waits on EMX after Send, so that Undo can take it
/// back: the person's setting in the web client, ten seconds unless
/// changed there, 0 for none.
export function undoSeconds(prefs: Record<string, unknown> | null | undefined): number {
  const v = prefs?.undoSend;
  return typeof v === 'number' && Number.isInteger(v) && v >= 0 && v <= 30 ? v : 10;
}

/// The times Send later offers, in local time: later today at six when
/// that is two hours off or more, tomorrow at eight, and Monday at eight
/// unless that is tomorrow.
export function laterChoices(now: Date): { label: string; at: Date }[] {
  const at = (days: number, hour: number): Date =>
    new Date(now.getFullYear(), now.getMonth(), now.getDate() + days, hour, 0, 0, 0);
  const out: { label: string; at: Date }[] = [];
  const evening = at(0, 18);
  if (evening.getTime() - now.getTime() >= 2 * 3600_000) out.push({ label: 'Later today', at: evening });
  out.push({ label: 'Tomorrow morning', at: at(1, 8) });
  // Days until the next Monday, 1 to 7.
  const toMonday = ((8 - now.getDay()) % 7) || 7;
  if (toMonday > 1) out.push({ label: 'Monday morning', at: at(toMonday, 8) });
  return out;
}

/// A time picked in a `datetime-local` field, read in local time; null
/// when it is not one, past, or more than a year ahead, which EMX does
/// not hold a message for.
export function pickedTime(value: string, now: Date): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value);
  if (!m) return null;
  const d = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], 0, 0);
  if (d.getMonth() !== +m[2] - 1 || d.getDate() !== +m[3]) return null;
  if (d.getTime() <= now.getTime() || d.getTime() > now.getTime() + 365 * 86400_000) return null;
  return d;
}

/// A local time as a `datetime-local` field writes it.
export function localInput(d: Date): string {
  const p = (n: number): string => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

// --- Snippets ---------------------------------------------------------------

/// A snippet being asked for just before the caret: `;;` and the first
/// letters of its name. Answers where the `;;` starts and the letters.
export function snippetAsked(before: string): { start: number; query: string } | null {
  const m = /;;([\p{L}\p{N}_-]{0,40})$/u.exec(before);
  return m ? { start: m.index, query: m[1] } : null;
}

/// The snippets whose names start with `query`, then those that contain
/// it, each group by name.
export function snippetsFor<T extends { name: string }>(list: T[], query: string): T[] {
  const q = query.toLowerCase();
  const byName = (a: T, b: T): number => a.name.localeCompare(b.name);
  const first = list.filter((s) => s.name.toLowerCase().startsWith(q)).sort(byName);
  const then = list.filter((s) => !s.name.toLowerCase().startsWith(q) && s.name.toLowerCase().includes(q)).sort(byName);
  return [...first, ...then];
}

/// The name of the first recipient of a To field: as written in front of
/// the address, or made from the address (`anna.muster@` is Anna
/// Muster); "Muster, Anna" is turned round.
export function recipientName(to: string): { name: string; firstName: string } {
  const first = (to.split(/[,;](?=(?:[^"]*"[^"]*")*[^"]*$)/)[0] ?? '').trim();
  const m = /^"?([^"<]*?)"?\s*<([^<>]+)>$/.exec(first);
  let name = m ? m[1].trim() : '';
  const address = (m ? m[2] : first).trim();
  if (!name && address.includes('@')) {
    const words = address
      .split('@')[0]
      .replace(/\+.*$/, '')
      .split(/[._-]+/)
      .filter((w) => /^\p{L}+$/u.test(w));
    name = words.map((w) => w[0].toUpperCase() + w.slice(1)).join(' ');
  }
  const turned = /^([^,]+),\s*(.+)$/.exec(name);
  if (turned) name = `${turned[2]} ${turned[1]}`;
  return { name, firstName: name.split(/\s+/)[0] ?? '' };
}

/// A snippet's text with `{firstName}`, `{name}` and `{myName}` filled in.
/// A value that is not known is left out, and with it the space before
/// it when punctuation follows: "Hello {firstName}," is "Hello,".
export function fillSnippet(text: string, values: { firstName: string; name: string; myName: string }): string {
  return text.replace(/( ?)\{(firstName|name|myName)\}/g, (whole, space: string, key: keyof typeof values, at: number) => {
    const v = values[key];
    if (v) return space + v;
    return /^[,.!?;:]/.test(text.slice(at + whole.length)) ? '' : space;
  });
}
