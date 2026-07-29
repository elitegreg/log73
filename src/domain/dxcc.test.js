import assert from 'node:assert/strict';
import test from 'node:test';
import {
  callsignPrefix,
  dxccContinent,
  dxccLabel,
  lookupDxcc,
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

test('callsignPrefix follows WPX prefix rules', () => {
  for (const [callsign, expected] of [
    ['W7DX', 'W7'],
    ['OL25LP', 'OL25'],
    ['DL60CHILD', 'DL60'],
    ['9A800VZ', '9A800'],
    ['DR2006Q', 'DR2006'],
    ['LY1000CW', 'LY1000'],
    ['KL7RA/WK9', 'WK9'],
    ['OE/K5ZD', 'OE0'],
    ['PA/N8BJQ', 'PA0'],
    ['XEFTJW', 'XE0'],
    ['F1ABC/MM', 'F1'],
    ['W9ABC/4', 'W4'],
    ['K1A/VE3', 'VE3'],
    ['EA8/K1A', 'EA8'],
    ['BAD/CALL/FORMAT', null],
    ['?', null],
  ]) {
    assert.equal(callsignPrefix(callsign), expected, callsign);
  }
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
  assert.deepEqual(lookupDxcc(TEST_DXCC, 'KP'), unitedStates);
  assert.deepEqual(lookupDxcc(TEST_DXCC, '4O'), montenegro);
  assert.equal(callsignPrefix('W7DX'), 'W7');
  assert.equal(lookupDxcc(TEST_DXCC, 'W7DX').primary_prefix, 'K');
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
