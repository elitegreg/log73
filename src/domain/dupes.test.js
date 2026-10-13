import assert from 'node:assert/strict';
import test from 'node:test';
import {
  loadCallsignFixture,
  loadDupeKeysFixture,
} from '../../tests/fixtures/domain/loadFixture.js';
import { dupeIdentity } from './callsign.js';
import {
  dupeAlertText,
  dupeAlertTextForAdif,
  dupeKey,
  dupeModeKey,
  modeClassKey,
} from './dupes.js';

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

test('shared fixture dupe identities match', () => {
  const fixture = loadCallsignFixture();
  assert.equal(fixture.version, 1);
  for (const kase of fixture.cases) {
    assert.equal(
      dupeIdentity(kase.input),
      kase.dupe_identity,
      `identity for ${kase.input}`,
    );
  }
});

test('shared dupe-keys fixture matches', () => {
  const fixture = loadDupeKeysFixture();
  assert.equal(fixture.version, 1);
  assert.ok(fixture.cases.length > 0);
  for (const kase of fixture.cases) {
    const settings = { scoring: { dupe_key: kase.dupe_key } };
    const keys = kase.contacts.map((adif) => dupeKey(settings, { adif }));
    assert.deepEqual(keys, kase.keys, `keys for ${kase.name}`);
  }
});

test('dupeModeKey groups digital modes and keeps SSB and FM distinct', () => {
  assert.equal(dupeModeKey('CW'), 'CW');
  assert.equal(dupeModeKey('CW-R'), 'CW');
  assert.equal(dupeModeKey('SSB'), 'SSB');
  assert.equal(dupeModeKey('FM'), 'FM');
  for (const mode of ['AM', 'PKT', 'RTTY', 'DATA', 'FT8', 'PSK']) {
    assert.equal(dupeModeKey(mode), 'DATA', mode);
  }
  assert.equal(dupeModeKey(' rtty '), 'DATA');
  assert.equal(dupeModeKey(null), '');
});

test('modeClassKey shares phone classes and passes digital modes through', () => {
  assert.equal(modeClassKey('CW'), 'CW');
  assert.equal(modeClassKey('CW-R'), 'CW');
  for (const mode of ['SSB', 'FM', 'AM']) {
    assert.equal(modeClassKey(mode), 'PHONE', mode);
  }
  for (const mode of ['PKT', 'RTTY', 'DATA', 'FT8']) {
    assert.equal(modeClassKey(mode), mode, mode);
  }
  assert.equal(modeClassKey(null), '');
});

test('dupeAlertText distinguishes operators sharing an operating prefix', () => {
  const prefixSettings = {
    scoring: { dupe_key: ['CALL', 'BAND', 'MODE'] },
  };
  const currentContact = contact({
    CALL: 'VE3/K1ABC',
    BAND: '20m',
    MODE: 'CW',
  });

  assert.equal(
    dupeAlertText(prefixSettings, currentContact, [
      contact({ CALL: 'VE3/N2XYZ', BAND: '20m', MODE: 'CW' }),
    ]),
    '',
  );
  assert.equal(
    dupeAlertText(prefixSettings, currentContact, [
      contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW' }),
    ]),
    'Dupe',
  );
});

test('dupeAlertText separates CW and SSB under a MODE_CLASS key', () => {
  const settings = { scoring: { dupe_key: ['CALL', 'BAND', 'MODE_CLASS'] } };
  const currentContact = contact({
    CALL: 'K1ABC',
    BAND: '20m',
    MODE: 'CW',
  });

  assert.equal(
    dupeAlertText(settings, currentContact, [
      contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'SSB' }),
    ]),
    '',
  );
  assert.equal(
    dupeAlertText(settings, currentContact, [
      contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW-R' }),
    ]),
    'Dupe',
  );
});

test('dupeAlertText shares PHONE classes under a MODE_CLASS key', () => {
  const settings = { scoring: { dupe_key: ['CALL', 'BAND', 'MODE_CLASS'] } };

  assert.equal(
    dupeAlertText(
      settings,
      contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'SSB' }),
      [contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'FM' })],
    ),
    'Dupe',
  );
});

test('dupeAlertText matches digital modes under a MODE key', () => {
  const settings = { scoring: { dupe_key: ['CALL', 'BAND', 'MODE'] } };

  assert.equal(
    dupeAlertText(
      settings,
      contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'PKT' }),
      [contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'RTTY' })],
    ),
    'Dupe',
  );
  assert.equal(
    dupeAlertText(
      settings,
      contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW' }),
      [contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'SSB' })],
    ),
    '',
  );
});

test('dupeAlertText reports possible dupes when the exchange differs', () => {
  const settings = {
    scoring: { dupe_key: ['CALL', 'BAND', 'MODE_CLASS', 'SRX_STRING'] },
  };

  assert.equal(
    dupeAlertText(
      settings,
      contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW', SRX_STRING: 'SC' }),
      [contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'CW', SRX_STRING: 'NC' })],
    ),
    'Possible Dupe',
  );
});

test('dupeAlertText reports possible dupes across digital modes', () => {
  const settings = {
    scoring: { dupe_key: ['CALL', 'BAND', 'MODE', 'SRX_STRING'] },
  };

  assert.equal(
    dupeAlertText(
      settings,
      contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'PKT', SRX_STRING: 'SC' }),
      [contact({ CALL: 'K1ABC', BAND: '20m', MODE: 'RTTY', SRX_STRING: 'NC' })],
    ),
    'Possible Dupe',
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
