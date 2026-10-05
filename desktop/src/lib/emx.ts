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
}

export function describe(e: unknown): string {
  if (e && typeof e === 'object' && 'message' in e) return String((e as Failure).message);
  return String(e);
}

export function retryable(e: unknown): boolean {
  return !!(e && typeof e === 'object' && (e as Failure).retryable);
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
  me: () => invoke<Me>('me'),
  mailboxes: (account: string) => invoke<Mailbox[]>('mailboxes', { account }),
  messages: async (account: string, mailbox: string, cursor = '') => {
    const p = await invoke<{ messages: Message[]; cursor: string }>('messages', { account, mailbox, cursor });
    return { messages: (p.messages ?? []).map(whole), cursor: p.cursor ?? '' };
  },
  message: async (account: string, id: string) => wholeFull(await invoke<FullMessage>('message', { account, id })),
  thread: async (account: string, id: string) => (await invoke<Message[]>('thread', { account, id })).map(whole),
  search: async (account: string, query: string) => (await invoke<Message[]>('search', { account, query })).map(whole),
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
  send: (draft: Draft) => invoke<number>('send', { draft }),
  contacts: (account: string, state: string) => invoke('contacts', { account, contactState: state }),
  setContact: (account: string, address: string, state: string) =>
    invoke<void>('set_contact', { account, address, contactState: state }),
  onChange: (fn: (n: { account: string; modseq: number }) => void): Promise<UnlistenFn> =>
    listen<{ account: string; modseq: number }>('emx://change', (e) => fn(e.payload))
};
