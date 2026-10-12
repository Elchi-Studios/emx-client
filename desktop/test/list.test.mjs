// SPDX-License-Identifier: AGPL-3.0-only
//
// The message list's rules, run with `npm test` (Node 22.18 or newer
// reads the TypeScript directly).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { appendPage, applyToFolder, applyToResults, deletesForGood, Latest, unique, withKeyword } from '../src/lib/list.ts';

const msg = (id, receivedAt, mailboxId = 'inbox', keywords = []) => ({ id, receivedAt, mailboxId, keywords });
const ids = (list) => list.map((m) => m.id);

test('a page never brings a second copy of a message', () => {
  const loaded = [msg('c', '2026-10-03'), msg('b', '2026-10-02')];
  const page = [msg('b', '2026-10-02'), msg('a', '2026-10-01')];
  assert.deepEqual(ids(appendPage(loaded, page)), ['c', 'b', 'a']);
  assert.deepEqual(ids(unique([msg('x', '1'), msg('x', '1'), msg('y', '1')])), ['x', 'y']);
});

test('a change below the loaded part waits for its page', () => {
  // Fifty loaded, more left; a message from the next page is flagged on
  // the phone. It must not be put at the end, where the next page would
  // bring it a second time.
  const loaded = [msg('c', '2026-10-03'), msg('b', '2026-10-02')];
  const old = msg('a', '2026-09-01', 'inbox', ['$flagged']);
  let list = applyToFolder(loaded, [old], [], 'inbox', true);
  assert.deepEqual(ids(list), ['c', 'b']);
  list = appendPage(list, [old]);
  assert.deepEqual(ids(list), ['c', 'b', 'a']);
  // Without more pages, the loaded part is the whole folder.
  assert.deepEqual(ids(applyToFolder(loaded, [old], [], 'inbox', false)), ['c', 'b', 'a']);
});

test('changes replace, remove and add within the loaded part', () => {
  const loaded = [msg('c', '2026-10-03'), msg('b', '2026-10-02'), msg('a', '2026-10-01')];
  const list = applyToFolder(
    loaded,
    [msg('d', '2026-10-04'), msg('b', '2026-10-02', 'archive'), msg('c', '2026-10-03', 'inbox', ['$seen']), msg('d', '2026-10-04')],
    ['a'],
    'inbox',
    true
  );
  assert.deepEqual(ids(list), ['d', 'c']);
  assert.deepEqual(list[1].keywords, ['$seen']);
  // A list that already has a duplicate comes out without it.
  assert.deepEqual(ids(applyToFolder([msg('x', '2'), msg('x', '2')], [], [], 'inbox', false)), ['x']);
});

test('search results take changes but no new messages', () => {
  const found = [msg('b', '2026-10-02', 'archive'), msg('a', '2026-10-01')];
  const list = applyToResults(found, [msg('a', '2026-10-01', 'trash'), msg('z', '2026-10-09')], ['b']);
  assert.deepEqual(ids(list), ['a']);
  assert.equal(list[0].mailboxId, 'trash');
});

test('keywords are set at most once', () => {
  assert.deepEqual(withKeyword(['$seen', '$flagged'], '$flagged', true), ['$seen', '$flagged']);
  assert.deepEqual(withKeyword(['$flagged', '$flagged'], '$flagged', false), []);
});

test('only a message in Trash or Junk is deleted for good', () => {
  const boxes = [
    { id: 'i', role: 'inbox' },
    { id: 't', role: 'trash' },
    { id: 'j', role: 'junk' },
    { id: 'x', role: '' }
  ];
  // Searching from Trash finds an Inbox message: it goes to Trash.
  assert.equal(deletesForGood({ mailboxId: 'i' }, boxes), false);
  assert.equal(deletesForGood({ mailboxId: 'x' }, boxes), false);
  assert.equal(deletesForGood({ mailboxId: 'gone' }, boxes), false);
  assert.equal(deletesForGood({ mailboxId: 't' }, boxes), true);
  assert.equal(deletesForGood({ mailboxId: 'j' }, boxes), true);
});

test('only the newest answer is applied', async () => {
  const views = new Latest();
  const shown = [];
  const ask = async (name, ms) => {
    const n = views.next();
    await new Promise((r) => setTimeout(r, ms));
    if (views.is(n)) shown.push(name);
  };
  // Inbox is clicked first and answers last.
  await Promise.all([ask('inbox', 30), ask('archive', 5)]);
  assert.deepEqual(shown, ['archive']);
  // A further page continues the current view, and a newer view stops it.
  const n = views.current;
  assert.ok(views.is(n));
  views.next();
  assert.ok(!views.is(n));
});
