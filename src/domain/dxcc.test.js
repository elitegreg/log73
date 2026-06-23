import assert from 'node:assert/strict';
import test from 'node:test';
import {
  callsignPrefix,
  dxccLabel,
  lookupDxcc,
  splitCallsign,
} from './dxcc.js';

const TEST_DXCC = {
  entities: [
    {
      country_name: 'Testland',
      cq_zone: 10,
      itu_zone: 20,
      continent: 'EU',
      latitude: 50,
      longitude: -10,
      utc_offset: -1,
      primary_prefix: 'T1',
    },
    {
      country_name: 'Montenegro',
      cq_zone: 15,
      itu_zone: 28,
      continent: 'EU',
      latitude: 42.5,
      longitude: -19.28,
      utc_offset: -1,
      primary_prefix: '4O',
    },
  ],
  rules: [
    { pattern: 'T1', exact: false, entity_index: 0 },
    {
      pattern: 'TA',
      exact: false,
      entity_index: 0,
      cq_zone: 11,
      itu_zone: 21,
      continent: 'AF',
      latitude: 51,
      longitude: 11,
      utc_offset: 2,
    },
    { pattern: 'T1ABC', exact: true, entity_index: 0 },
    { pattern: '4O', exact: false, entity_index: 1 },
  ],
};

test('splitCallsign returns prefix number and suffix', () => {
  assert.deepEqual(splitCallsign('KB1AWN'), {
    prefix: 'KB',
    number: '1',
    suffix: 'AWN',
  });
  assert.deepEqual(splitCallsign('NK12A'), {
    prefix: 'NK',
    number: '12',
    suffix: 'A',
  });
  assert.deepEqual(splitCallsign('4O9A'), {
    prefix: '4O',
    number: '9',
    suffix: 'A',
  });
  assert.equal(splitCallsign('KP'), null);
  assert.equal(splitCallsign('4O'), null);
});

test('callsignPrefix follows the digit-delimited prefix rule', () => {
  assert.equal(callsignPrefix('KP2M'), 'KP');
  assert.equal(callsignPrefix('4O9A'), '4O');
  assert.equal(callsignPrefix('KP'), null);
  assert.equal(callsignPrefix('4O'), null);
});

test('lookupDxcc prefers exact matches and then longest prefixes', () => {
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'T1ABC'), {
    country_name: 'Testland',
    cq_zone: 10,
    itu_zone: 20,
    continent: 'EU',
    latitude: 50,
    longitude: -10,
    utc_offset: -1,
    primary_prefix: 'T1',
  });
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'TA9ZZ'), {
    country_name: 'Testland',
    cq_zone: 11,
    itu_zone: 21,
    continent: 'AF',
    latitude: 51,
    longitude: 11,
    utc_offset: 2,
    primary_prefix: 'T1',
  });
  assert.deepEqual(lookupDxcc(TEST_DXCC, '4O9A'), {
    country_name: 'Montenegro',
    cq_zone: 15,
    itu_zone: 28,
    continent: 'EU',
    latitude: 42.5,
    longitude: -19.28,
    utc_offset: -1,
    primary_prefix: '4O',
  });
  assert.equal(lookupDxcc(TEST_DXCC, 'KP'), null);
});

test('dxccLabel formats country and continent', () => {
  assert.equal(
    dxccLabel({ country_name: 'Montenegro', continent: 'eu' }),
    'Montenegro EU',
  );
  assert.equal(dxccLabel(null), '');
});
