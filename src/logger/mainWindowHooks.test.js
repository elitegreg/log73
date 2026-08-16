import assert from 'node:assert/strict';
import test from 'node:test';
import {
  activeMessageKeysFromRequests,
  addActiveMessageRequest,
  removeActiveMessageRequest,
} from './hooks/messageSendingState.js';
import {
  textSendingIsEnabled,
  textWordForMode,
} from './hooks/useTextDialog.js';

test('message sending state accumulates and clears active keys by request id', () => {
  let requests = new Map();
  requests = addActiveMessageRequest(requests, 'a', ['F1', 'F2']);
  requests = addActiveMessageRequest(requests, 'b', ['F2', 'F3']);

  assert.deepEqual(
    [...activeMessageKeysFromRequests(requests)],
    ['F1', 'F2', 'F3'],
  );

  requests = removeActiveMessageRequest(requests, 'a');
  assert.deepEqual([...activeMessageKeysFromRequests(requests)], ['F2', 'F3']);

  requests = removeActiveMessageRequest(requests, 'b');
  assert.deepEqual([...activeMessageKeysFromRequests(requests)], []);
});

test('text dialog eligibility includes CW and targeted digital operation', () => {
  assert.equal(textSendingIsEnabled('CW', false), true);
  assert.equal(textSendingIsEnabled('CW-R', false), true);
  assert.equal(textSendingIsEnabled('DATA', true), true);
  assert.equal(textSendingIsEnabled('RTTY', true), true);
  assert.equal(textSendingIsEnabled('DATA', false), false);
  assert.equal(textSendingIsEnabled('SSB', true), false);
});

test('text dialog uppercases CW but preserves FLDigi letter case', () => {
  assert.equal(textWordForMode(' cq Test ', 'CW'), 'CQ TEST');
  assert.equal(textWordForMode(' cq Test ', 'DATA'), 'cq Test');
  assert.equal(textWordForMode(' cq Test ', 'RTTY'), 'cq Test');
});
