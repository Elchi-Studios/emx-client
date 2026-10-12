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
