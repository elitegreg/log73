import assert from 'node:assert/strict';
import test from 'node:test';
import {
  callsignFilterPrefix,
  callsignPrefix,
  dxccContinent,
  dxccLabel,
  lookupDxcc,
  splitCallsign,
} from './dxcc.js';

const testland = {
  country_name: 'Testland',
  adif: 123,
  cq_zone: 10,
  itu_zone: 20,
  continent: 'EU',
  latitude: 50,
  longitude: -10,
  utc_offset: -1,
  primary_prefix: 'T1',
  waedc_cq_list: false,
};
const montenegro = {
  country_name: 'Montenegro',
  adif: 514,
  cq_zone: 15,
  itu_zone: 28,
  continent: 'EU',
  latitude: 42.5,
  longitude: -19.28,
  utc_offset: -1,
  primary_prefix: '4O',
  waedc_cq_list: false,
};
const canada = {
  country_name: 'Canada',
  adif: 1,
  cq_zone: 4,
  itu_zone: 9,
  continent: 'NA',
  latitude: 56,
  longitude: 96,
  utc_offset: 5,
  primary_prefix: 'VE3',
  waedc_cq_list: false,
};
const unitedStates = {
  country_name: 'United States',
  adif: 291,
  cq_zone: 5,
  itu_zone: 8,
  continent: 'NA',
  latitude: 38,
  longitude: 97,
  utc_offset: 5,
  primary_prefix: 'K',
  waedc_cq_list: false,
};
const shetland = {
  country_name: 'Shetland Islands',
  adif: 279,
  cq_zone: 14,
  itu_zone: 27,
  continent: 'EU',
  latitude: 60.5,
  longitude: 1.5,
  utc_offset: 0,
  primary_prefix: 'GM/s',
  waedc_cq_list: true,
};
const bouvet = {
  country_name: 'Bouvet',
  adif: 24,
  cq_zone: 38,
  itu_zone: 67,
  continent: 'AF',
  latitude: -54.42,
  longitude: -3.38,
  utc_offset: -1,
  primary_prefix: '3Y/b',
  waedc_cq_list: false,
};

const TEST_DXCC = {
  entities: [testland, montenegro, canada, unitedStates, shetland, bouvet],
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
    { pattern: 'VE3', exact: false, entity_index: 2 },
    { pattern: 'K', exact: false, entity_index: 3 },
    { pattern: 'N', exact: false, entity_index: 3 },
    { pattern: 'W', exact: false, entity_index: 3 },
    { pattern: 'GM0AVR', exact: true, entity_index: 3 },
    { pattern: 'GM0AVR', exact: true, entity_index: 4 },
    { pattern: '3Y/LB5SH', exact: true, entity_index: 5 },
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

test('callsignFilterPrefix waits for a full prefix plus number', () => {
  assert.equal(callsignFilterPrefix('K'), '');
  assert.equal(callsignFilterPrefix('KP'), '');
  assert.equal(callsignFilterPrefix('4O'), '');
  assert.equal(callsignFilterPrefix('K1'), 'K1');
  assert.equal(callsignFilterPrefix('KP2'), 'KP2');
  assert.equal(callsignFilterPrefix('4O9A'), '4O9');
});

test('lookupDxcc prefers exact matches and then longest prefixes', () => {
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'T1ABC'), testland);
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'TA9ZZ'), {
    ...testland,
    cq_zone: 11,
    itu_zone: 21,
    continent: 'AF',
    latitude: 51,
    longitude: 11,
    utc_offset: 2,
  });
  assert.deepEqual(lookupDxcc(TEST_DXCC, '4O9A'), montenegro);
  assert.equal(lookupDxcc(TEST_DXCC, 'KP'), null);
});

test('lookupDxcc resolves slash-prefixed and slash-suffixed DXCCs', () => {
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'VE3/NG4M'), canada);
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'NG4M/VE3'), canada);
});

test('lookupDxcc checks exact full callsigns before slash resolution', () => {
  assert.deepEqual(lookupDxcc(TEST_DXCC, '3Y/LB5SH'), bouvet);
});

test('lookupDxcc returns WAEDC/CQ entity flag', () => {
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'GM0AVR'), shetland);
});

test('lookupDxcc ignores common slash suffixes and falls back to the root callsign', () => {
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'NG4M/P'), unitedStates);
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'NG4M/MM'), unitedStates);
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'NG4M/QRP'), unitedStates);
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'NG4M/1'), unitedStates);
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'NG4M/XYZ'), unitedStates);
});

test('dxccLabel formats country and continent', () => {
  assert.equal(
    dxccLabel({ country_name: 'Montenegro', continent: 'eu' }),
    'Montenegro EU',
  );
  assert.equal(dxccLabel(null), '');
});

test('dxccContinent normalizes known continents and returns null when unknown', () => {
  assert.equal(dxccContinent({ continent: ' eu ' }), 'EU');
  assert.equal(dxccContinent(null), null);
});
