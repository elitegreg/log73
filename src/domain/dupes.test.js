import assert from 'node:assert/strict';
import test from 'node:test';
import { dupeAlertText, dupeAlertTextForAdif } from './dupes.js';

const settings = {
  scoring: { dupe_key: ['CALL', 'BAND', 'MODE', 'SRX_STRING'] },
};

function contact(adif) {
  return { adif };
}

test('dupeAlertText is blank without dupe key fields', () => {
  assert.equal(
    dupeAlertText({ scoring: { dupe_key: [] } }, contact({ CALL: 'K1ABC' }), [
      contact({ CALL: 'K1ABC' }),
    ]),
    '',
  );
});

test('dupeAlertText detects exact dupes', () => {
  const currentContact = contact({
    CALL: 'K1ABC',
    BAND: '20m',
    MODE: 'CW',
    SRX_STRING: 'SC',
  });
  const historicContacts = [
    contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW', SRX_STRING: 'NC' }),
    contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW', SRX_STRING: 'SC' }),
  ];

  assert.equal(
    dupeAlertText(settings, currentContact, historicContacts),
    'Dupe',
  );
});

test('dupeAlertTextForAdif detects a live Field Day entry against logged contacts', () => {
  const fieldDaySettings = {
    scoring: { dupe_key: ['CALL', 'BAND', 'MODE'] },
  };
  const currentAdif = {
    CALL: 'W4MEL',
    BAND: '40m',
    FREQ: 7000000,
    MODE: 'CW',
  };
  const historicContacts = [
    contact({
      CALL: 'W4MEL',
      BAND: '40m',
      FREQ: 7000000,
      MODE: 'CW',
    }),
  ];

  assert.equal(
    dupeAlertTextForAdif(fieldDaySettings, currentAdif, historicContacts),
    'Dupe',
  );
});

test('dupeAlertText detects possible dupes before exact exchange is known', () => {
  const currentContact = contact({
    CALL: 'K1ABC',
    BAND: '20m',
    MODE: 'CW',
    SRX_STRING: '',
  });
  const historicContacts = [
    contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW', SRX_STRING: 'SC' }),
  ];

  assert.equal(
    dupeAlertText(settings, currentContact, historicContacts),
    'Possible Dupe',
  );
});

test('dupeAlertText possible key uses only call, band, and mode fields from dupe key', () => {
  const callOnlyPossibleSettings = {
    scoring: { dupe_key: ['CALL', 'SRX_STRING'] },
  };

  assert.equal(
    dupeAlertText(
      callOnlyPossibleSettings,
      contact({ CALL: 'K1ABC', SRX_STRING: 'SC' }),
      [contact({ CALL: 'K1ABC', SRX_STRING: 'NC' })],
    ),
    'Possible Dupe',
  );
});

test('dupeAlertText normalizes callsigns and canonical field values like scoring', () => {
  const mappedSettings = {
    scoring: { dupe_key: ['CALL', 'BAND', 'MODE'] },
  };

  assert.equal(
    dupeAlertText(
      mappedSettings,
      contact({ CALL: 'k1abc/p', BAND: '20m', MODE: 'cw' }),
      [contact({ CALL: 'K1ABC', BAND: '20M', MODE: 'CW' })],
    ),
    'Dupe',
  );
});

test('dupeAlertText keeps scanning newest-first contacts after unrelated callsigns', () => {
  const currentContact = contact({
    CALL: 'K1ABC',
    BAND: '20m',
    MODE: 'CW',
    SRX_STRING: 'SC',
  });
  const historicContacts = [
    contact({ CALL: 'K9ZZZ', BAND: '20m', MODE: 'CW', SRX_STRING: 'SC' }),
    contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW', SRX_STRING: 'NC' }),
    contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW', SRX_STRING: 'SC' }),
  ];

  assert.equal(
    dupeAlertText(settings, currentContact, historicContacts),
    'Dupe',
  );
});
