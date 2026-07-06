import assert from 'node:assert/strict';
import test from 'node:test';
import {
  activeMessageKeysFromRequests,
  addActiveMessageRequest,
  removeActiveMessageRequest,
} from './hooks/messageSendingState.js';

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
