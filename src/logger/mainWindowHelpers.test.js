import assert from 'node:assert/strict';
import test from 'node:test';
import {
  availableModeOptions,
  cwActiveTimeoutMs,
  modeIsCw,
  nextCwWpm,
  typedModeFromCallsignInput,
} from './mainWindowHelpers.js';

test('availableModeOptions includes concrete selectable modes', () => {
  assert.deepEqual(availableModeOptions({ allowed_modes: ['cw'] }), [
    'CW',
    'CW-R',
    'SSB',
    'FM',
    'FT8',
    'JT65',
    'JT9',
    'MFSK',
    'PSK',
    'RTTY',
  ]);
});

test('typedModeFromCallsignInput matches exact mode tokens only', () => {
  const settings = { allowed_modes: ['cw', 'rtty', 'ssb'] };

  assert.equal(typedModeFromCallsignInput('cw', settings), 'CW');
  assert.equal(typedModeFromCallsignInput('cw-r', settings), 'CW-R');
  assert.equal(typedModeFromCallsignInput('ft8', settings), 'FT8');
  assert.equal(typedModeFromCallsignInput('RTTY', settings), 'RTTY');
  assert.equal(typedModeFromCallsignInput(' fm ', {}), 'FM');
  assert.equal(typedModeFromCallsignInput('AM', settings), null);
  assert.equal(typedModeFromCallsignInput('K1CW', settings), null);
  assert.equal(typedModeFromCallsignInput('ss', settings), null);
  assert.equal(typedModeFromCallsignInput('', settings), null);
});

test('modeIsCw treats CW-R as CW', () => {
  assert.equal(modeIsCw('CW'), true);
  assert.equal(modeIsCw('CW-R'), true);
  assert.equal(modeIsCw('RTTY'), false);
});

test('nextCwWpm clamps page-up and page-down changes to valid range', () => {
  assert.equal(nextCwWpm(20, 1), 21);
  assert.equal(nextCwWpm(20, -1), 19);
  assert.equal(nextCwWpm(60, 1), 60);
  assert.equal(nextCwWpm(5, -1), 5);
  assert.equal(nextCwWpm(Number.NaN, 1), 21);
});

test('cwActiveTimeoutMs waits for completion-capable keyers', () => {
  assert.equal(cwActiveTimeoutMs('winkeyer'), 30000);
  assert.equal(cwActiveTimeoutMs('cat'), 30000);
  assert.equal(cwActiveTimeoutMs('serial'), 30000);
  assert.equal(cwActiveTimeoutMs('none'), 500);
});
