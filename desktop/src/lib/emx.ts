// SPDX-License-Identifier: AGPL-3.0-only

// The window's side of the app: every call goes to the Rust side by
// name, which holds the token and talks to EMX.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface Address {
  name: string;
  address: string;
}
export interface Rights {
  own: boolean;
  read: boolean;
  write: boolean;
  delete: boolean;
  sendAs: boolean;
  sendOnBehalf: boolean;
  manage: boolean;
}
export interface Account {
  id: string;
  kind: string;
  name: string;
  address: string;
  rights: Rights;
}
export interface SendFrom {
  address: string;
  name: string;
  mode: string;
  primary: boolean;
}
export interface Me {
  id: string;
  name: string;
  role: string;
  tenant: { id: string; name: string; plan: string };
  accounts: Account[];
  sendFrom: SendFrom[];
  prefs: Record<string, unknown>;
  /** What the person may send; `maxMessageBytes` is the whole message,
      `maxRecipients` To, Cc and Bcc of one message together. */
  limits?: { maxMessageBytes?: number; maxRecipients?: number; hourlyRecipients?: number };
}
export interface Mailbox {
  id: string;
  parentId: string | null;
  name: string;
  role: string;
  sortOrder: number;
  total: number;
  unseen: number;
  modseq: number;
}
export interface Message {
  id: string;
  mailboxId: string;
  threadId: string;
  receivedAt: string;
  sentAt: string | null;
  subject: string;
  from: Address;
  to: string[];
  cc: string[];
  snippet: string;
  hasAttachments: boolean;
  sealed: boolean;
  keywords: string[];
  size: number;
  dmarc: string;
  modseq: number;
}
export interface Attachment {
  part: string;
  filename: string;
  contentType: string;
  size: number;
  inline: boolean;
  contentId: string;
}
export interface Body {
  text: string;
  html: string;
  remoteImages: number;
  attachments: Attachment[];
  replyTo: Address[];
  messageId: string;
  inReplyTo: string;
  references: string[];
  sealed: boolean;
}
export interface FullMessage {
  message: Message;
  body: Body;
}
export interface Changes {
  updated: Message[];
  destroyed: string[];
  modseq: number;
  hasMore: boolean;
}
export interface Draft {
  from: string;
  to: string;
  cc?: string;
  bcc?: string;
  subject: string;
  text: string;
  html?: string;
  inReplyTo?: string;
  references?: string[];
  attachments?: { filename: string; contentType: string; data: string }[];
}
export interface Failure {
  code: string;
  message: string;
  /** The same call may work in a moment: the network or EMX is away. */
  retryable?: boolean;
  /** The seconds EMX asked to wait, when it named them. */
  retryAfter?: number | null;
}

export interface Person {
  id: string;
  name: string;
}
/// A draft saved on EMX. Its body is the composer's own JSON; a shared
/// mailbox's drafts are written by everyone who may send from it.
export interface SavedDraft {
  id: string;
  account: string;
  version: number;
  body: Record<string, unknown>;
  createdAt?: string | null;
  createdBy?: Person | null;
  updatedAt: string;
  editedBy?: Person | null;
  sentAt?: string | null;
  sentBy?: Person | null;
}
/// What a change of a draft made on a version came to: done, or refused
/// with the draft as it is now because somebody changed or sent it.
export type OnVersion<T> =
  | { outcome: 'done'; value: T }
  | { outcome: 'changed'; value: SavedDraft }
  | { outcome: 'sent'; value: SavedDraft };
/// How a send holds its message on EMX: seconds for Undo, or a time.
export interface Hold {
  undoSeconds?: number;
  sendAt?: string;
  remindDays?: number;
}
/// A message held on EMX: to be taken back for a few seconds, or until a
/// time. One that could not go is `failed`, and its draft a draft again.
export interface Held {
  id: string;
  draft: string;
  account: string;
  kind: 'undo' | 'later' | string;
  sendAt: string;
  state: 'held' | 'failed' | string;
  errorCode: string;
  errorMessage: string;
  createdAt: string;
  sender: Person;
  message?: SavedDraft | null;
}
export interface DraftSent {
  sent: boolean;
  recipients: number;
  held: boolean;
  outbox?: Held | null;
  replayed: boolean;
}
export interface FileData {
  filename: string;
  contentType: string;
  data: string;
}
export interface Cancelled {
  draft: SavedDraft;
  attachments: FileData[];
}
export interface Snippet {
  id: string;
  account: string;
  body: Record<string, unknown>;
}

/// A new idempotency key: 128 random bits. EMX answers a send with a key
/// it has seen from the same person within a day with the first answer,
/// instead of sending again.
export function newKey(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return 'emx-desktop-' + Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

export function describe(e: unknown): string {
  if (e && typeof e === 'object' && 'message' in e) return String((e as Failure).message);
  return String(e);
}

/// Whether EMX no longer takes the sign-in: the token expired or was
/// revoked. Only signing in again helps.
export function refused(e: unknown): boolean {
  // Only a 401 is: `try_later`, EMX failing to check the sign-in for a
  // moment, is no sign-out.
  const code = e && typeof e === 'object' ? (e as Failure).code : '';
  return code === 'bad_token' || code === 'signed_out';
}

export function retryable(e: unknown): boolean {
  return !!(e && typeof e === 'object' && (e as Failure).retryable);
}

/// The heading for a failure that passes: the network is away, EMX
/// refuses for a moment because of too many requests, or EMX is not
/// answering.
export function outage(e: unknown): string {
  const code = e && typeof e === 'object' ? (e as Failure).code : '';
  if (code === 'offline') return 'No connection';
  if (code === 'rate_limited') return 'Too many requests';
  return 'EMX is not answering';
}

// The service leaves empty lists out of its JSON. The Rust side fills
// them in, and a message is made whole here too, so nothing downstream
// has to ask.
export function whole(m: Message): Message {
  return { ...m, to: m.to ?? [], cc: m.cc ?? [], keywords: m.keywords ?? [], from: m.from ?? { name: '', address: '' } };
}
export function wholeFull(f: FullMessage): FullMessage {
  const b = f.body ?? ({} as Body);
  return {
    message: whole(f.message),
    body: { ...b, attachments: b.attachments ?? [], replyTo: b.replyTo ?? [], references: b.references ?? [], text: b.text ?? '', html: b.html ?? '' }
  };
}

export const emx = {
  signIn: (baseUrl: string, token: string) => invoke<Me>('sign_in', { baseUrl, token }),
  resume: () => invoke<Me | null>('resume'),
  signOut: () => invoke<void>('sign_out'),
  /// Starts the live updates of the sign-in, once the window listens.
  follow: () => invoke<void>('follow'),
  me: () => invoke<Me>('me'),
  mailboxes: (account: string) => invoke<Mailbox[]>('mailboxes', { account }),
  messages: async (account: string, mailbox: string, cursor = '') => {
    const p = await invoke<{ messages: Message[]; cursor: string }>('messages', { account, mailbox, cursor });
    return { messages: (p.messages ?? []).map(whole), cursor: p.cursor ?? '' };
  },
  message: async (account: string, id: string) => wholeFull(await invoke<FullMessage>('message', { account, id })),
  thread: async (account: string, id: string) => (await invoke<Message[]>('thread', { account, id })).map(whole),
  search: async (account: string, query: string, cursor = '') => {
    const p = await invoke<{ messages: Message[]; cursor: string }>('search', { account, query, cursor });
    return { messages: (p.messages ?? []).map(whole), cursor: p.cursor ?? '' };
  },
  changes: async (account: string, since: number) => {
    const c = await invoke<Changes>('changes', { account, since });
    return { ...c, updated: (c.updated ?? []).map(whole), destroyed: c.destroyed ?? [] };
  },
  keywords: (account: string, ids: string[], add: string[], remove: string[]) =>
    invoke<string[]>('keywords', { account, ids, add, remove }),
  move: (account: string, ids: string[], to: string) => invoke<string[]>('move_messages', { account, ids, to }),
  delete: (account: string, ids: string[]) => invoke<string[]>('delete_messages', { account, ids }),
  snooze: (account: string, ids: string[], until: string) => invoke<string[]>('snooze', { account, ids, until }),
  part: (account: string, id: string, part: string) =>
    invoke<{ content_type: string; base64: string }>('part', { account, id, part }),
  savePart: (account: string, id: string, part: string, filename: string) =>
    invoke<string>('save_part', { account, id, part, filename }),
  send: (draft: Draft, idempotencyKey: string) => invoke<number>('send', { draft, idempotencyKey }),
  /// The message as received, saved to Downloads under the name EMX
  /// gives it.
  saveRaw: (account: string, id: string) => invoke<string>('save_raw', { account, id }),
  drafts: (account: string) => invoke<SavedDraft[]>('drafts', { account }),
  draft: (account: string, id: string) => invoke<SavedDraft>('draft', { account, id }),
  saveDraft: (account: string, id: string, version: number, body: Record<string, unknown>) =>
    invoke<OnVersion<SavedDraft>>('save_draft', { account, id, version, body }),
  deleteDraft: (account: string, id: string, version: number | null) =>
    invoke<OnVersion<null>>('delete_draft', { account, id, version }),
  sendDraft: (account: string, id: string, version: number, message: Draft, hold: Hold, idempotencyKey: string) =>
    invoke<OnVersion<DraftSent>>('send_draft', { account, id, version, message, hold, idempotencyKey }),
  outbox: (account: string) => invoke<Held[]>('outbox', { account }),
  cancelHeld: (account: string, id: string) => invoke<Cancelled>('cancel_held', { account, id }),
  rescheduleHeld: (account: string, id: string, sendAt: string) => invoke<Held>('reschedule_held', { account, id, sendAt }),
  snippets: (account: string) => invoke<{ snippets: Snippet[]; editable: boolean }>('snippets', { account }),
  contacts: (account: string, state: string) => invoke('contacts', { account, contactState: state }),
  setContact: (account: string, address: string, state: string) =>
    invoke<void>('set_contact', { account, address, contactState: state }),
  onChange: (fn: (n: { account: string; modseq: number }) => void): Promise<UnlistenFn> =>
    listen<{ account: string; modseq: number }>('emx://change', (e) => fn(e.payload)),
  /// A draft or a held message of an account changed: read them again.
  onDrafts: (fn: (n: { account: string }) => void): Promise<UnlistenFn> =>
    listen<{ account: string }>('emx://drafts', (e) => fn(e.payload)),
  /// The app stopped following an account because EMX refused it.
  onRefused: (fn: (f: Failure) => void): Promise<UnlistenFn> => listen<Failure>('emx://refused', (e) => fn(e.payload))
};
