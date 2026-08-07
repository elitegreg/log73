import assert from 'node:assert/strict';
import test from 'node:test';
import {
  nextAvailableWsjtxPort,
  wsjtxDataModeLocked,
  wsjtxTargetControlVisible,
} from './wsjtx.js';

test('nextAvailableWsjtxPort skips ports reserved by enabled radios', () => {
  assert.equal(
    nextAvailableWsjtxPort([
      { wsjtx_enabled: true, wsjtx_port: 2237 },
      { wsjtx_enabled: true, wsjtx_port: 2238 },
      { wsjtx_enabled: false, wsjtx_port: 2239 },
    ]),
    2239,
  );
});

test('nextAvailableWsjtxPort ignores malformed input and honors the minimum', () => {
  assert.equal(nextAvailableWsjtxPort(null, 1), 1024);
});

test('wsjtxDataModeLocked requires both enabled configuration and DATA mode', () => {
  assert.equal(wsjtxDataModeLocked({ wsjtx_enabled: true }, ' data '), true);
  assert.equal(wsjtxDataModeLocked({ wsjtx_enabled: true }, 'CW'), false);
  assert.equal(wsjtxDataModeLocked({ wsjtx_enabled: false }, 'DATA'), false);
});

test('WSJT-X target replaces ESM only for enabled DATA mode', () => {
  assert.equal(wsjtxTargetControlVisible(true, 'DATA'), true);
  assert.equal(wsjtxTargetControlVisible(true, 'CW'), false);
  assert.equal(wsjtxTargetControlVisible(false, 'DATA'), false);
});
