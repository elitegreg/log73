import assert from 'node:assert/strict';
import test from 'node:test';
import {
  availableModeOptions,
  nextCwWpm,
  typedModeFromCallsignInput,
} from './mainWindowHelpers.js';

test('availableModeOptions prefers allowed contest modes when present', () => {
  assert.deepEqual(
    availableModeOptions({ allowed_modes: ['cw', 'rtty', 'ssb'] }),
    ['CW', 'RTTY', 'SSB'],
  );
  assert.deepEqual(availableModeOptions({}), ['CW', 'SSB', 'FM']);
});

test('typedModeFromCallsignInput matches exact mode tokens only', () => {
  const settings = { allowed_modes: ['cw', 'rtty', 'ssb'] };

  assert.equal(typedModeFromCallsignInput('cw', settings), 'CW');
  assert.equal(typedModeFromCallsignInput('RTTY', settings), 'RTTY');
  assert.equal(typedModeFromCallsignInput(' fm ', {}), 'FM');
  assert.equal(typedModeFromCallsignInput('K1CW', settings), null);
  assert.equal(typedModeFromCallsignInput('ss', settings), null);
  assert.equal(typedModeFromCallsignInput('', settings), null);
});

test('nextCwWpm clamps page-up and page-down changes to valid range', () => {
  assert.equal(nextCwWpm(20, 1), 21);
  assert.equal(nextCwWpm(20, -1), 19);
  assert.equal(nextCwWpm(60, 1), 60);
  assert.equal(nextCwWpm(5, -1), 5);
  assert.equal(nextCwWpm(Number.NaN, 1), 21);
});
