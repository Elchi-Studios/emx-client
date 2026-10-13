// SPDX-License-Identifier: AGPL-3.0-only
//
// What the window makes from a message: the document its frame shows, and
// who a reply goes to. Without Svelte, so that it can be tested on its own.

/// The document for a message's frame, around HTML the service has
/// sanitised. Every link opens in a new window, which the app hands to
/// the system. HTML mail is written for a white page, its colours
/// included, so it is shown on one in dark mode too, as the web client
/// does: dark text of its own would be unreadable on a dark panel.
export function frame(body: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="color-scheme" content="light"><base target="_blank"><style>
      :root{color-scheme:light}
      body{margin:0;padding:12px 16px;font:14px/1.5 -apple-system,system-ui,sans-serif;color:#1a1a1a;background:#fff;word-wrap:break-word}
      img{max-width:100%;height:auto} blockquote{border-left:3px solid #ccc;margin:8px 0;padding-left:10px;color:#666} pre{white-space:pre-wrap}
      </style></head><body>${body}</body></html>`;
}

/// The bare address in `Name <address>` or `address`, in lower case, for
/// comparing.
export function bareAddress(s: string): string {
  const m = s.match(/<([^<>]*)>\s*$/);
  return (m ? m[1] : s).trim().toLowerCase();
}

/// Who a reply goes to. `to` and `cc` are the message's recipients,
/// `replyTo` its Reply-To or its sender, and `own` every address the
/// person sends from. Reply all adds the other recipients, without the
/// person's own addresses, each address once. A reply to the person's
/// own message goes to whom it went to.
export function replyRecipients(
  m: { from: string; to: string[]; cc: string[]; replyTo: string[] },
  own: string[],
  all: boolean
): string[] {
  const mine = new Set(own.map(bareAddress));
  const fromMe = mine.has(bareAddress(m.from));
  const first = fromMe ? m.to : m.replyTo.length ? m.replyTo : [m.from];
  const rest = all ? (fromMe ? m.cc : [...m.to, ...m.cc]) : [];
  const out: string[] = [];
  const seen = new Set<string>();
  for (const a of first) {
    const k = bareAddress(a);
    if (k && !seen.has(k)) {
      seen.add(k);
      out.push(a);
    }
  }
  for (const a of rest) {
    const k = bareAddress(a);
    if (k && !seen.has(k) && !mine.has(k)) {
      seen.add(k);
      out.push(a);
    }
  }
  // A message from the person to only themselves still gets an answer.
  if (out.length === 0 && first.length) out.push(first[0]);
  return out;
}

const named: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
  shy: '',
  ndash: '-',
  mdash: '-',
  hellip: '...',
  laquo: '«',
  raquo: '»',
  lsquo: "'",
  rsquo: "'",
  ldquo: '"',
  rdquo: '"',
  bdquo: '"',
  euro: '€',
  copy: '©',
  reg: '®',
  auml: 'ä',
  ouml: 'ö',
  uuml: 'ü',
  Auml: 'Ä',
  Ouml: 'Ö',
  Uuml: 'Ü',
  szlig: 'ß',
  eacute: 'é',
  egrave: 'è',
  agrave: 'à',
  ccedil: 'ç'
};

/// Character references decoded once each: `&amp;lt;` reads `&lt;`.
function decodeEntities(s: string): string {
  return s.replace(/&(#[xX][0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);/g, (whole, ref: string) => {
    if (ref[0] === '#') {
      const n = ref[1] === 'x' || ref[1] === 'X' ? parseInt(ref.slice(2), 16) : parseInt(ref.slice(1), 10);
      if (n === 13) return '';
      if (n === 160) return ' ';
      return n > 0 && n <= 0x10ffff && !(n >= 0xd800 && n <= 0xdfff) ? String.fromCodePoint(n) : whole;
    }
    return ref in named ? named[ref] : whole;
  });
}

/// The text of a message's HTML, for quoting in a reply when the message
/// has no text part: paragraphs, line breaks, list items and table rows
/// on lines of their own, links with their address, nothing of styles,
/// scripts or comments, and no more than one empty line in a row.
export function htmlToText(html: string): string {
  let s = html
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<(head|style|script|title)\b[\s\S]*?<\/\1\s*>/gi, '')
    .replace(/<a\b[^>]*?\bhref\s*=\s*"(https?:[^"]*)"[^>]*>([\s\S]*?)<\/a\s*>/gi, (_m, href: string, inner: string) => {
      const label = inner.replace(/<[^>]*>/g, '').trim();
      const url = decodeEntities(href);
      return label && decodeEntities(label) !== url ? `${inner} (${href})` : inner || href;
    })
    .replace(/<br\b[^>]*>/gi, '\n')
    .replace(/<li\b[^>]*>/gi, '\n- ')
    .replace(/<\/?(p|div|tr|h[1-6]|ul|ol|table|blockquote|section|article|header|footer|hr)\b[^>]*>/gi, '\n')
    .replace(/<\/t[dh]\s*>/gi, ' ')
    .replace(/<[^>]*>/g, '');
  s = decodeEntities(s);
  return s
    .split('\n')
    .map((l) => l.replace(/[ \t\f\v ]+/g, ' ').trim())
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

/// The text a reply quotes: the message's text part, or a text rendering
/// of its HTML when it has none, as most newsletters and many invoices.
export function quotable(body: { text: string; html: string }): string {
  return body.text.trim() ? body.text : htmlToText(body.html);
}

/// The size a message with attachments of `bytes` in all comes to as
/// mail: base64 makes three bytes four, in lines of 76 characters and a
/// line end, and each part has headers. The service measures this
/// against its limit.
export function encodedSize(bytes: number, parts: number): number {
  return Math.ceil(bytes / 3) * 4 * (78 / 76) + parts * 512;
}

/// Whether attachments of `sizes` fit a message of at most `max` bytes
/// with `text` besides; `max` 0 or missing means the service did not say.
export function fits(sizes: number[], text: string, max: number | undefined): boolean {
  if (!max) return true;
  const bytes = sizes.reduce((a, b) => a + b, 0);
  return encodedSize(bytes, sizes.length) + new TextEncoder().encode(text).length * 2 + 4096 <= max;
}

/// A reply's subject: `Re: ` in front, unless it is a reply already,
/// in English or German (`Re:`, `AW:`, `Antw:`). A forward (`Fwd:`,
/// `WG:`) is answered with `Re: WG: ...`.
export function replySubject(subject: string): string {
  return /^\s*(re|aw|antw)\s*:/i.test(subject) ? subject : 'Re: ' + subject;
}

/// One recipient as the service reads it: a display name with any of
/// the characters that mean something in an address quoted, and none
/// when it is only the address again.
export function formatRecipient(name: string, address: string): string {
  const n = name.trim();
  if (!n || n.toLowerCase() === address.toLowerCase()) return address;
  return /^[\p{L}\p{N} !#$%&'*+\-/=?^_`{|}~.]+$/u.test(n) && !/^\.|\.$|\.\./.test(n)
    ? `${n} <${address}>`
    : `"${n.replace(/(["\\])/g, '\\$1')}" <${address}>`;
}

/// A To or Cc field as typed, made into the comma separated list the
/// service reads: `;` between addresses, as Outlook writes them, and
/// bare addresses with only spaces between them are separated by
/// commas; display names that need it are quoted. Anything it does not
/// recognise is left for the service to judge.
export function normalizeRecipients(field: string): string {
  return recipientList(field).join(', ');
}

/// The recipients of a To, Cc or Bcc field as typed, one each, as
/// [`normalizeRecipients`] writes them.
export function recipientList(field: string): string[] {
  const pieces: string[] = [];
  let cur = '';
  let quoted = false;
  let angle = false;
  for (let i = 0; i < field.length; i++) {
    const c = field[i];
    if (c === '\\' && quoted && i + 1 < field.length) {
      cur += c + field[++i];
      continue;
    }
    if (c === '"') quoted = !quoted;
    else if (!quoted && c === '<') angle = true;
    else if (!quoted && c === '>') angle = false;
    if (!quoted && !angle && (c === ',' || c === ';')) {
      pieces.push(cur);
      cur = '';
      continue;
    }
    cur += c;
  }
  pieces.push(cur);
  const out: string[] = [];
  for (const raw of pieces) {
    const p = raw.trim();
    if (!p) continue;
    const named = p.match(/^(.*?)\s*<\s*([^<>\s]+@[^<>\s]+)\s*>$/);
    if (named) {
      let name = named[1].trim();
      if (name.length >= 2 && name.startsWith('"') && name.endsWith('"')) name = name.slice(1, -1).replace(/\\(.)/g, '$1');
      out.push(formatRecipient(name, named[2]));
      continue;
    }
    const words = p.split(/\s+/);
    if (words.length > 1 && words.every((w) => /^[^\s@<>"]+@[^\s@<>"]+$/.test(w))) out.push(...words);
    else out.push(p);
  }
  return out;
}

/// How many recipients the fields name together, each address once.
export function countRecipients(...fields: string[]): number {
  return new Set(fields.flatMap(recipientList).map(bareAddress)).size;
}
