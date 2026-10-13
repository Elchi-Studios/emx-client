// SPDX-License-Identifier: AGPL-3.0-only

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  bodyFor,
  fieldsOf,
  fillSnippet,
  isSealed,
  laterChoices,
  localInput,
  pickedTime,
  recipientName,
  sameFields,
  SendKey,
  snippetAsked,
  snippetsFor,
  undoSeconds
} from '../src/lib/compose.ts';

test('a draft keeps the fields the web client wrote', () => {
  const base = { from: 'contact@elchi.dev', to: 'anna@example.ch', subject: 'Offerte', text: 'Hallo', answers: 'x9', remind: 3, forward: { id: 'm1' } };
  const f = fieldsOf(base);
  assert.equal(f.cc, '');
  const saved = bodyFor(base, { ...f, text: 'Hallo Anna' });
  assert.equal(saved.text, 'Hallo Anna');
  assert.equal(saved.answers, 'x9');
  assert.equal(saved.remind, 3);
  assert.deepEqual(saved.forward, { id: 'm1' });
  assert.equal(saved.cc, '');
  assert.ok(!sameFields(f, fieldsOf(saved)));
  assert.ok(sameFields(f, fieldsOf(bodyFor(base, f))));
  assert.deepEqual(fieldsOf(null).references, undefined);
  assert.deepEqual(fieldsOf({ references: ['a', 3, 'b'] }).references, ['a', 'b']);
  assert.ok(isSealed({ sealed: 'YWdl' }));
  assert.ok(!isSealed({ text: 'x' }));
});

test('a send keeps its key until what it sends changes', () => {
  const k = new SendKey();
  const a = k.for('{"to":"anna"}');
  assert.match(a, /^emx-desktop-[0-9a-f]{32}$/);
  assert.equal(k.for('{"to":"anna"}'), a, 'Send pressed again');
  const b = k.for('{"to":"anna, marco"}');
  assert.notEqual(b, a, 'another message, another key');
  k.forget();
  assert.notEqual(k.for('{"to":"anna, marco"}'), b);
});

test('the undo time follows the setting', () => {
  assert.equal(undoSeconds({}), 10);
  assert.equal(undoSeconds({ undoSend: 0 }), 0);
  assert.equal(undoSeconds({ undoSend: 30 }), 30);
  assert.equal(undoSeconds({ undoSend: 31 }), 10);
  assert.equal(undoSeconds({ undoSend: '5' }), 10);
  assert.equal(undoSeconds(null), 10);
});

test('send later offers times ahead', () => {
  // A Tuesday morning.
  const tue = new Date(2026, 9, 13, 10, 0);
  assert.deepEqual(
    laterChoices(tue).map((c) => [c.label, localInput(c.at)]),
    [
      ['Later today', '2026-10-13T18:00'],
      ['Tomorrow morning', '2026-10-14T08:00'],
      ['Monday morning', '2026-10-19T08:00']
    ]
  );
  // Late on a Sunday: no later today, and Monday is tomorrow.
  const sun = new Date(2026, 9, 18, 17, 0);
  assert.deepEqual(
    laterChoices(sun).map((c) => c.label),
    ['Tomorrow morning']
  );
  // On a Monday, the next Monday.
  const mon = new Date(2026, 9, 19, 7, 0);
  assert.equal(localInput(laterChoices(mon).at(-1).at), '2026-10-26T08:00');
});

test('a picked time is ahead and within a year', () => {
  const now = new Date(2026, 9, 13, 10, 0);
  assert.equal(localInput(pickedTime('2026-10-14T07:30', now)), '2026-10-14T07:30');
  assert.equal(pickedTime('2026-10-13T09:00', now), null);
  assert.equal(pickedTime('2027-11-01T09:00', now), null);
  assert.equal(pickedTime('2026-02-30T09:00', now), null);
  assert.equal(pickedTime('tomorrow', now), null);
});

test('a snippet is asked for with ;;', () => {
  assert.deepEqual(snippetAsked('Hallo\n;;dan'), { start: 6, query: 'dan' });
  assert.deepEqual(snippetAsked(';;'), { start: 0, query: '' });
  assert.equal(snippetAsked('a;b'), null);
  assert.equal(snippetAsked(';;dan ke'), null);
  const list = [{ name: 'thanks' }, { name: 'meeting-thanks' }, { name: 'offer' }];
  assert.deepEqual(snippetsFor(list, 'than').map((s) => s.name), ['thanks', 'meeting-thanks']);
  assert.deepEqual(snippetsFor(list, '').map((s) => s.name), ['meeting-thanks', 'offer', 'thanks']);
});

test('a snippet is filled in for the first recipient', () => {
  assert.deepEqual(recipientName('Anna Beispiel <anna@beispiel-ag.ch>, marco@x.ch'), { name: 'Anna Beispiel', firstName: 'Anna' });
  assert.deepEqual(recipientName('"Muster, Anna" <a@x.ch>'), { name: 'Anna Muster', firstName: 'Anna' });
  assert.deepEqual(recipientName('anna.muster@example.ch'), { name: 'Anna Muster', firstName: 'Anna' });
  assert.deepEqual(recipientName('info@example.ch'), { name: 'Info', firstName: 'Info' });
  assert.deepEqual(recipientName('a1@example.ch'), { name: '', firstName: '' });
  assert.deepEqual(recipientName(''), { name: '', firstName: '' });
  const v = { firstName: 'Anna', name: 'Anna Muster', myName: 'Samuel' };
  assert.equal(fillSnippet('Hello {firstName},\n\nthank you!\n{myName}', v), 'Hello Anna,\n\nthank you!\nSamuel');
  assert.equal(fillSnippet('Hello {firstName},', { ...v, firstName: '' }), 'Hello,');
  assert.equal(fillSnippet('Dear {name} and team', { ...v, name: '' }), 'Dear  and team');
  assert.equal(fillSnippet('{other} stays', v), '{other} stays');
});
