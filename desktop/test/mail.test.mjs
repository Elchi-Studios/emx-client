// SPDX-License-Identifier: AGPL-3.0-only

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bareAddress, encodedSize, fits, formatRecipient, frame, htmlToText, normalizeRecipients, quotable, replyRecipients, replySubject } from '../src/lib/mail.ts';

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

test('mail is shown on a white page in either theme', () => {
  const doc = frame('<p>Hallo</p>');
  assert.match(doc, /<meta name="color-scheme" content="light">/);
  assert.match(doc, /background:#fff/);
  assert.doesNotMatch(doc, /CanvasText|transparent/);
  assert.match(doc, /<base target="_blank">/);
});

test('an html-only message is quoted as text', () => {
  const html =
    '<html><head><style>p{color:red}</style></head><body><!-- x --><p>Guten Tag&nbsp;Herr M&uuml;ller,</p>' +
    '<p>Er sagte &#34;ja&#34; &amp;lt;b&amp;gt;<br>zweite Zeile</p><ul><li>eins</li><li>zwei</li></ul>' +
    '<table><tr><td>Betrag</td><td>CHF 120.00</td></tr></table>' +
    '<p><a href="https://shop.ch/pay?a=1&amp;b=2" target="_blank">Jetzt bezahlen</a> oder <a href="https://shop.ch">https://shop.ch</a></p></body></html>';
  assert.equal(
    htmlToText(html),
    'Guten Tag Herr Müller,\n\nEr sagte "ja" &lt;b&gt;\nzweite Zeile\n\n- eins\n- zwei\n\nBetrag CHF 120.00\n\nJetzt bezahlen (https://shop.ch/pay?a=1&b=2) oder https://shop.ch'
  );
  assert.equal(quotable({ text: 'plain', html }), 'plain');
  assert.equal(quotable({ text: '  \n', html: '<p>a</p>' }), 'a');
  assert.equal(quotable({ text: '', html: '' }), '');
});

test('attachments are measured as mail measures them', () => {
  const max = 50 * 1024 * 1024;
  assert.equal(fits([30 * 1024 * 1024], '', max), true);
  // 40 MB of files come to about 54 MB as mail.
  assert.equal(fits([20 * 1024 * 1024, 20 * 1024 * 1024], '', max), false);
  assert.equal(fits([40 * 1024 * 1024], '', 0), true);
  assert.equal(fits([], 'Hallo', undefined), true);
  assert.ok(encodedSize(3, 1) >= 4);
});

test('a reply subject gets one prefix, in English or German', () => {
  assert.equal(replySubject('Offerte'), 'Re: Offerte');
  assert.equal(replySubject('RE: Offerte'), 'RE: Offerte');
  assert.equal(replySubject('AW: Offerte'), 'AW: Offerte');
  assert.equal(replySubject('Antw: Offerte'), 'Antw: Offerte');
  assert.equal(replySubject('WG: Offerte'), 'Re: WG: Offerte');
  assert.equal(replySubject('Fwd: Offerte'), 'Re: Fwd: Offerte');
  assert.equal(replySubject('Rechnung: Mai'), 'Re: Rechnung: Mai');
});

test('recipients are written the way the service reads them', () => {
  assert.equal(normalizeRecipients('a@b.ch; c@d.ch'), 'a@b.ch, c@d.ch');
  assert.equal(normalizeRecipients('a@b.ch c@d.ch'), 'a@b.ch, c@d.ch');
  assert.equal(normalizeRecipients(' a@b.ch ,, c@d.ch ;'), 'a@b.ch, c@d.ch');
  assert.equal(normalizeRecipients('noreply@shop.ch <noreply@shop.ch>'), 'noreply@shop.ch');
  assert.equal(normalizeRecipients('Support [Ticket] <s@b.ch>'), '"Support [Ticket]" <s@b.ch>');
  assert.equal(normalizeRecipients('"Anna; Team" <a@b.ch>; c@d.ch'), '"Anna; Team" <a@b.ch>, c@d.ch');
  assert.equal(normalizeRecipients('Team: Sales <s@b.ch>'), '"Team: Sales" <s@b.ch>');
  assert.equal(normalizeRecipients('Hans Müller <h@b.ch>'), 'Hans Müller <h@b.ch>');
  assert.equal(normalizeRecipients('Dr. Hans Müller <h@b.ch>'), 'Dr. Hans Müller <h@b.ch>');
  assert.equal(normalizeRecipients('Müller & Co <h@b.ch>'), 'Müller & Co <h@b.ch>');
  assert.equal(normalizeRecipients('"Say \\"hi\\"" <h@b.ch>'), '"Say \\"hi\\"" <h@b.ch>');
  assert.equal(normalizeRecipients(''), '');
  // Something it does not recognise goes as typed, for the service to judge.
  assert.equal(normalizeRecipients('not an address'), 'not an address');
  assert.equal(formatRecipient('', 'a@b.ch'), 'a@b.ch');
});
