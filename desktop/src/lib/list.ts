// SPDX-License-Identifier: AGPL-3.0-only
//
// How a loaded list of messages takes in a further page and the changes
// from the feed. Kept apart from the window's state, without Svelte, so
// that it can be tested on its own.

import type { Message } from './emx';

/// Newest first, as the service lists them; ties by id, so that the
/// order never depends on how the list was put together.
export function newestFirst(a: Message, b: Message): number {
  if (a.receivedAt !== b.receivedAt) return a.receivedAt < b.receivedAt ? 1 : -1;
  return a.id < b.id ? 1 : a.id > b.id ? -1 : 0;
}

/// The list without a second copy of any message: the window keys its
/// rows by id, and one id twice stops the list from updating at all.
/// The first copy stays.
export function unique(list: Message[]): Message[] {
  const seen = new Set<string>();
  return list.filter((m) => (seen.has(m.id) ? false : (seen.add(m.id), true)));
}

/// A further page after the loaded ones. A message can be in both: one
/// that a change brought in, or one that moved up while scrolling.
export function appendPage(list: Message[], page: Message[]): Message[] {
  const have = new Set(list.map((m) => m.id));
  return [...list, ...page.filter((m) => !have.has(m.id))];
}

/// The loaded part of a folder after a batch of changes. A message that
/// left the folder goes, one that is still in it is replaced, and one
/// that came in is added only where the loaded part reaches: when more
/// pages are left (`more`), a message older than the last loaded one
/// belongs to a page not loaded yet, which brings it when it comes.
export function applyToFolder(
  list: Message[],
  updated: Message[],
  destroyed: string[],
  mailboxId: string,
  more: boolean
): Message[] {
  const gone = new Set(destroyed);
  const byId = new Map(updated.map((u) => [u.id, u]));
  const oldest = list.length ? list[list.length - 1] : null;
  const reaches = (u: Message): boolean => !more || !oldest || newestFirst(u, oldest) <= 0;
  const next: Message[] = [];
  for (const m of list) {
    if (gone.has(m.id)) continue;
    const u = byId.get(m.id);
    if (!u) next.push(m);
    else if (u.mailboxId === mailboxId) next.push(u);
  }
  const have = new Set(next.map((m) => m.id));
  for (const u of byId.values()) {
    if (!have.has(u.id) && !gone.has(u.id) && u.mailboxId === mailboxId && reaches(u)) next.push(u);
  }
  return unique(next).sort(newestFirst);
}

/// Search results after a batch of changes: what is gone goes, what
/// changed is replaced, and nothing is added, since only the service
/// knows what matches.
export function applyToResults(list: Message[], updated: Message[], destroyed: string[]): Message[] {
  const gone = new Set(destroyed);
  const byId = new Map(updated.map((u) => [u.id, u]));
  return unique(list.filter((m) => !gone.has(m.id)).map((m) => byId.get(m.id) ?? m));
}

/// A message's keywords with one added or taken away, each at most once.
export function withKeyword(keywords: string[], keyword: string, on: boolean): string[] {
  const rest = keywords.filter((k) => k !== keyword);
  return on ? [...rest, keyword] : rest;
}

/// Whether Delete removes a message for good: only when the message
/// itself is in Trash or Junk. The open folder does not decide, since a
/// search finds messages in every folder.
export function deletesForGood(m: Pick<Message, 'mailboxId'>, mailboxes: { id: string; role: string }[]): boolean {
  const role = mailboxes.find((b) => b.id === m.mailboxId)?.role;
  return role === 'trash' || role === 'junk';
}

/// Hands out numbers for requests of one kind. Only the answer to the
/// newest one is applied; an answer that comes back after a newer
/// request was made is dropped, so a slow answer never overwrites a
/// faster, later one.
export class Latest {
  private n = 0;

  /// A new request, which makes every earlier one stale.
  next(): number {
    return ++this.n;
  }

  /// The number of the newest request, to continue it, as a further page
  /// continues the list it belongs to.
  get current(): number {
    return this.n;
  }

  /// Whether request `n` is still the newest.
  is(n: number): boolean {
    return n === this.n;
  }
}

/// The message to show after `id` leaves the list: the one below it, or
/// the one above when it was the last, or none when it was alone.
export function neighbour(list: { id: string }[], id: string): string | null {
  const i = list.findIndex((m) => m.id === id);
  if (i < 0) return null;
  return list[i + 1]?.id ?? list[i - 1]?.id ?? null;
}
