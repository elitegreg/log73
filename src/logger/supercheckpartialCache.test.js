import assert from 'node:assert/strict';
import test from 'node:test';
import {
  mergeSupercheckpartialCallsigns,
  normalizeSupercheckpartialCallsign,
} from './supercheckpartialCache.js';

test('normalizeSupercheckpartialCallsign trims and uppercases values', () => {
  assert.equal(normalizeSupercheckpartialCallsign(' k1abc '), 'K1ABC');
  assert.equal(normalizeSupercheckpartialCallsign(''), null);
  assert.equal(normalizeSupercheckpartialCallsign(null), null);
});

test('mergeSupercheckpartialCallsigns preserves order while deduping normalized callsigns', () => {
  assert.deepEqual(
    mergeSupercheckpartialCallsigns(
      ['K1ABC', ' w1aw '],
      ['k1abc', '', 'N0CALL', 'W1AW', null],
    ),
    ['K1ABC', 'W1AW', 'N0CALL'],
  );
});

test('mergeSupercheckpartialCallsigns accepts missing arrays', () => {
  assert.deepEqual(mergeSupercheckpartialCallsigns(undefined, ['k1abc']), [
    'K1ABC',
  ]);
  assert.deepEqual(mergeSupercheckpartialCallsigns(['k1abc'], undefined), [
    'K1ABC',
  ]);
});
