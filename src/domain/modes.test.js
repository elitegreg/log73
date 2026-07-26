import assert from 'node:assert/strict';
import test from 'node:test';
import {
  LOGGER_MODE_OPTIONS,
  adifModeForLoggerMode,
  isSelectableMode,
  modeIsCw,
  modeIsPhone,
  normalizeLoggerMode,
} from './modes.js';

test('LOGGER_MODE_OPTIONS lists concrete selectable modes', () => {
  assert.deepEqual(LOGGER_MODE_OPTIONS, [
    'CW',
    'CW-R',
    'SSB',
    'FM',
    'AM',
    'DATA',
    'RTTY',
  ]);
});

test('mode helpers normalize and classify modes', () => {
  assert.equal(normalizeLoggerMode(' cw-r '), 'CW-R');
  assert.equal(isSelectableMode('DATA'), true);
  assert.equal(isSelectableMode('AM'), true);
  assert.equal(modeIsCw('CW'), true);
  assert.equal(modeIsCw('cw-r'), true);
  assert.equal(modeIsCw('RTTY'), false);
  assert.equal(modeIsPhone('SSB'), true);
  assert.equal(modeIsPhone('fm'), true);
  assert.equal(modeIsPhone(' am '), true);
  assert.equal(modeIsPhone('CW'), false);
});

test('adifModeForLoggerMode maps logger modes to ADIF modes', () => {
  assert.equal(adifModeForLoggerMode('CW'), 'CW');
  assert.equal(adifModeForLoggerMode('CW-R'), 'CW');
  assert.equal(adifModeForLoggerMode('DATA'), 'PKT');
  assert.equal(adifModeForLoggerMode('RTTY'), 'RTTY');
});
