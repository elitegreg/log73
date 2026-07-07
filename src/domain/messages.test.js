import assert from 'node:assert/strict';
import test from 'node:test';
import {
  actionFromTemplate,
  messageActionForConfig,
  parseMessageEntries,
} from './messages.js';

const TEST_CONFIG = `
# RUN Messages
F1 Cq,CQ TEST
F12 Clear,{Action:Clear}
# S&P Messages
F12 Clear,CQ
`;

test('message helpers parse entries and action tokens', () => {
  const entries = parseMessageEntries(TEST_CONFIG);
  assert.equal(entries.length, 3);
  assert.equal(entries[0].mode, 'run');
  assert.equal(entries[0].key, 'F1');
  assert.equal(entries[0].label, 'Cq');
  assert.equal(actionFromTemplate('{Action:Clear}'), 'Clear');
  assert.equal(messageActionForConfig(TEST_CONFIG, 'run', 'F12'), 'Clear');
  assert.equal(messageActionForConfig(TEST_CONFIG, 's&p', 'F12'), null);
});
