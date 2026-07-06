import assert from 'node:assert/strict';
import test from 'node:test';
import { dupeAlertText } from './dupes.js';

const settings = {
  dupe_key: ['CALL', 'BAND', 'MODE', 'SRX_STRING'],
  qso_column_fields: {},
};

function contact(adif) {
  return { adif };
}

test('dupeAlertText is blank without dupe key fields', () => {
  assert.equal(
    dupeAlertText({ dupe_key: [] }, contact({ CALL: 'K1ABC' }), [
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
    dupe_key: ['CALL', 'SRX_STRING'],
    qso_column_fields: {},
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

test('dupeAlertText normalizes callsigns and mapped field values like scoring', () => {
  const mappedSettings = {
    dupe_key: ['CALL', 'Band', 'Mode'],
    qso_column_fields: {
      Band: 'BAND',
      Mode: 'MODE',
    },
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

  assert.equal(dupeAlertText(settings, currentContact, historicContacts), 'Dupe');
});
