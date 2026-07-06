import test from 'node:test';
import assert from 'node:assert/strict';
import {
  RUN_MESSAGE_MODE,
  SEARCH_AND_POUNCE_MESSAGE_MODE,
  normalizeMessageMode,
  parseMessageModeSectionHeader,
} from './messageModes.js';

test('normalizeMessageMode accepts search-and-pounce aliases', () => {
  assert.equal(normalizeMessageMode('run'), RUN_MESSAGE_MODE);
  assert.equal(normalizeMessageMode('sp'), SEARCH_AND_POUNCE_MESSAGE_MODE);
  assert.equal(
    normalizeMessageMode('search_and_pounce'),
    SEARCH_AND_POUNCE_MESSAGE_MODE,
  );
  assert.equal(
    normalizeMessageMode('search and pounce'),
    SEARCH_AND_POUNCE_MESSAGE_MODE,
  );
});

test('parseMessageModeSectionHeader recognizes run and S&P headings', () => {
  assert.equal(parseMessageModeSectionHeader('# RUN Messages'), 'run');
  assert.equal(parseMessageModeSectionHeader('# S&P Messages'), 's&p');
  assert.equal(
    parseMessageModeSectionHeader('# Search and Pounce Messages'),
    's&p',
  );
});
