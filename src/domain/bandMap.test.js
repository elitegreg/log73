import assert from 'node:assert/strict';
import test from 'node:test';
import {
  BAND_MAP_IN_USE_CALLSIGN,
  BAND_MAP_VFO_CALLSIGN,
  addBandMapSpot,
  addCqBandMapSpot,
  addInUseBandMapSpot,
  bandMapRows,
  createBandMapSpotStore,
  formatBandMapKhz,
  frequencyTenthKhz,
  lastCqFrequencyForBand,
  nextBandMapSpotAbove,
  nextBandMapSpotBelow,
  removeBandMapSpot,
} from './bandMap.js';

function spot(id, frequencyHz, callDx, extras = {}) {
  return {
    id,
    frequency_hz: frequencyHz,
    call_dx: callDx,
    call_de: 'N0CALL',
    ...extras,
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
  assert.equal(rows[0].callsign, 'K1ABC (DX)');
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
    ['W9XYZ (DX)', BAND_MAP_VFO_CALLSIGN, 'K1ABC (DX)'],
  );
});

test('band map navigation finds the nearest callsign spot above and below VFO', () => {
  const store = addInUseBandMapSpot(
    addCqBandMapSpot(
      createBandMapSpotStore([
        spot(1, 14074100, 'N5DEF'),
        spot(2, 14074200, 'K1ABC'),
        spot(3, 14074400, 'W9XYZ'),
      ]),
      14074300,
      '20m',
      1,
      'K4',
    ),
    14074150,
  );

  assert.equal(nextBandMapSpotAbove(store, 14074150).call_dx, 'K1ABC');
  assert.equal(nextBandMapSpotBelow(store, 14074350).call_dx, 'K1ABC');
  assert.equal(nextBandMapSpotAbove(store, 14074400), null);
  assert.equal(nextBandMapSpotBelow(store, 14074100), null);
});

test('band map special spots display labels and track CQ frequencies per radio', () => {
  const store = addInUseBandMapSpot(
    addCqBandMapSpot(createBandMapSpotStore(), 14074000, '20m', 7, 'K4'),
    14075000,
  );
  const rows = bandMapRows(store, 14073000);

  assert.deepEqual(
    rows.filter((row) => row.type === 'spot').map((row) => row.callsign),
    ['*** CQ (K4) ***', BAND_MAP_IN_USE_CALLSIGN],
  );
  assert.equal(lastCqFrequencyForBand(store, '20m', 7), 14074000);
  assert.equal(lastCqFrequencyForBand(store, '20m', 8), null);
});

test('band map supports multiple in-use markers by frequency', () => {
  const store = addInUseBandMapSpot(
    addInUseBandMapSpot(
      addInUseBandMapSpot(createBandMapSpotStore(), 14074100),
      14074200,
    ),
    14074249,
  );

  assert.equal(store.sortedSpots.length, 2);
  assert.deepEqual(
    store.sortedSpots.map((currentSpot) => currentSpot.frequency_tenth_khz),
    [140741, 140742],
  );
});

test('band map frequency helpers round and format to one decimal kHz', () => {
  assert.equal(frequencyTenthKhz(14074245), 140742);
  assert.equal(formatBandMapKhz(frequencyTenthKhz(14074245)), '14074.2');
  assert.equal(formatBandMapKhz(frequencyTenthKhz(144000000)), '144000.0');
});

test('band map displays source suffixes for dx, rbn, and local spots', () => {
  const rows = bandMapRows(
    createBandMapSpotStore([
      spot(1, 14074000, 'K1ABC'),
      spot(2, 14074100, 'N1JB', { spot_type: 'rbn', source: 'rbn' }),
      spot(3, 14074200, 'W1AW', { spot_type: 'local', source: 'local' }),
    ]),
    14073000,
  );

  assert.deepEqual(
    rows.filter((row) => row.type === 'spot').map((row) => row.callsign),
    ['K1ABC (DX)', 'N1JB (RBN)', 'W1AW (LOCAL)'],
  );
});
