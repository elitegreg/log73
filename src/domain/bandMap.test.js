import assert from 'node:assert/strict';
import test from 'node:test';
import {
  BAND_MAP_VFO_CALLSIGN,
  addBandMapSpot,
  bandMapRows,
  createBandMapSpotStore,
  formatBandMapKhz,
  frequencyTenthKhz,
  removeBandMapSpot,
} from './bandMap.js';

function spot(id, frequencyHz, callDx) {
  return {
    id,
    frequency_hz: frequencyHz,
    call_dx: callDx,
    call_de: 'N0CALL',
  };
}

test('band map store sorts spots by rounded tenth-kHz frequency', () => {
  const store = createBandMapSpotStore([
    spot(1, 14074200, 'K1ABC'),
    spot(2, 7003100, 'W9XYZ'),
    spot(3, 14074100, 'N5DEF'),
  ]);

  assert.deepEqual(
    store.sortedSpots.map((currentSpot) => currentSpot.call_dx),
    ['W9XYZ', 'N5DEF', 'K1ABC'],
  );
});

test('band map delete removes a spot by id', () => {
  const store = removeBandMapSpot(
    createBandMapSpotStore([spot(1, 14074200, 'K1ABC')]),
    1,
  );

  assert.equal(store.sortedSpots.length, 0);
  assert.equal(store.spotsById.has('1'), false);
});

test('band map add replaces existing spot id', () => {
  const store = addBandMapSpot(
    createBandMapSpotStore([spot(1, 14074200, 'K1ABC')]),
    spot(1, 7003100, 'W9XYZ'),
  );

  assert.equal(store.sortedSpots.length, 1);
  assert.equal(store.sortedSpots[0].call_dx, 'W9XYZ');
  assert.equal(store.sortedSpots[0].frequency_tenth_khz, 70031);
});

test('band map rows mark a spot matching the rounded VFO frequency', () => {
  const rows = bandMapRows(
    createBandMapSpotStore([spot(1, 14074240, 'K1ABC')]),
    14074245,
  );

  assert.equal(rows.length, 1);
  assert.equal(rows[0].marker, '➜');
  assert.equal(rows[0].callsign, 'K1ABC');
});

test('band map rows insert VFO row when no spot matches', () => {
  const rows = bandMapRows(
    createBandMapSpotStore([
      spot(1, 7003100, 'W9XYZ'),
      spot(2, 14074200, 'K1ABC'),
    ]),
    10100100,
  );

  assert.deepEqual(
    rows.map((row) => row.callsign),
    ['W9XYZ', BAND_MAP_VFO_CALLSIGN, 'K1ABC'],
  );
});

test('band map frequency helpers round and format to one decimal kHz', () => {
  assert.equal(frequencyTenthKhz(14074245), 140742);
  assert.equal(formatBandMapKhz(frequencyTenthKhz(14074245)), '14074.2');
  assert.equal(formatBandMapKhz(frequencyTenthKhz(144000000)), '144000.0');
});
