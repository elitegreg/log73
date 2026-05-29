import assert from 'node:assert/strict';
import test from 'node:test';
import {
  LOGGER_MODE_OPTIONS,
  adifModeForLoggerMode,
  isSelectableMode,
  modeIsCw,
  normalizeLoggerMode,
} from './modes.js';

test('LOGGER_MODE_OPTIONS lists concrete selectable modes', () => {
  assert.deepEqual(LOGGER_MODE_OPTIONS, [
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

test('mode helpers normalize and classify modes', () => {
  assert.equal(normalizeLoggerMode(' cw-r '), 'CW-R');
  assert.equal(isSelectableMode('FT8'), true);
  assert.equal(isSelectableMode('AM'), false);
  assert.equal(modeIsCw('CW'), true);
  assert.equal(modeIsCw('cw-r'), true);
  assert.equal(modeIsCw('RTTY'), false);
});

test('adifModeForLoggerMode maps CW-R to CW', () => {
  assert.equal(adifModeForLoggerMode('CW'), 'CW');
  assert.equal(adifModeForLoggerMode('CW-R'), 'CW');
  assert.equal(adifModeForLoggerMode('FT8'), 'FT8');
});
