// SPDX-License-Identifier: AGPL-3.0-only
//
// What the window makes from a message: the document its frame shows, and
// who a reply goes to. Without Svelte, so that it can be tested on its own.

/// The document for a message's frame, around HTML the service has
/// sanitised. Every link opens in a new window, which the app hands to
/// the system.
export function frame(body: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><base target="_blank"><style>
      body{margin:0;padding:4px 0;font:14px/1.5 -apple-system,system-ui,sans-serif;color:CanvasText;background:transparent;word-wrap:break-word}
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
