// SPDX-License-Identifier: AGPL-3.0-only

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bareAddress, replyRecipients } from '../src/lib/mail.ts';

test('own addresses are matched whole, in any case', () => {
  const m = { from: 'anna@example.ch', to: ['s@x.ch', 'lukas@x.ch'], cc: ['S@X.CH', 'info@x.ch'], replyTo: [] };
  // `s@x.ch` is part of `lukas@x.ch`, and lukas must stay.
  assert.deepEqual(replyRecipients(m, ['s@x.ch'], true), ['anna@example.ch', 'lukas@x.ch', 'info@x.ch']);
  // An alias the person sends from is theirs too.
  assert.deepEqual(replyRecipients(m, ['s@x.ch', 'Info@x.ch'], true), ['anna@example.ch', 'lukas@x.ch']);
  assert.deepEqual(replyRecipients(m, ['s@x.ch'], false), ['anna@example.ch']);
});

test('a reply goes to Reply-To, each address once', () => {
  const m = { from: 'news@shop.ch', to: ['s@x.ch'], cc: ['help@shop.ch'], replyTo: ['help@shop.ch'] };
  assert.deepEqual(replyRecipients(m, ['s@x.ch'], true), ['help@shop.ch']);
});

test('a reply to the persons own message goes to its recipients', () => {
  const m = { from: 'S@x.ch', to: ['anna@example.ch'], cc: ['lukas@x.ch', 's@x.ch'], replyTo: [] };
  assert.deepEqual(replyRecipients(m, ['s@x.ch'], false), ['anna@example.ch']);
  assert.deepEqual(replyRecipients(m, ['s@x.ch'], true), ['anna@example.ch', 'lukas@x.ch']);
  // A note to oneself is answered to oneself.
  const self = { from: 's@x.ch', to: ['s@x.ch'], cc: [], replyTo: [] };
  assert.deepEqual(replyRecipients(self, ['s@x.ch'], true), ['s@x.ch']);
});

test('addresses compare without their names', () => {
  assert.equal(bareAddress('Anna Muster <Anna@Example.ch>'), 'anna@example.ch');
  assert.equal(bareAddress(' anna@example.ch '), 'anna@example.ch');
});
