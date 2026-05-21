import assert from 'node:assert/strict';
import test from 'node:test';
import { callsignCompletionMatches, exchangeCompletionMatches } from './completions.js';

test('callsign completions require at least three characters', () => {
  assert.deepEqual(callsignCompletionMatches(['K1ABC', 'N1XYZ'], 'K1'), []);
});

test('callsign completions match case-insensitive substrings', () => {
  assert.deepEqual(
    callsignCompletionMatches(['K1ABC', 'N1XYZ', 'W4CAE'], '1ab'),
    ['K1ABC'],
  );
});

test('callsign completions are limited', () => {
  const callsigns = Array.from({ length: 120 }, (_, index) => `K1A${String(index).padStart(3, '0')}`);
  assert.equal(callsignCompletionMatches(callsigns, 'K1A').length, 100);
  assert.equal(callsignCompletionMatches(callsigns, 'K1A', 5).length, 5);
});

test('exchange completions use valid values', () => {
  const field = { valid_values: ['ABBE', 'AIKE', 'SC', 'NC'] };
  assert.deepEqual(exchangeCompletionMatches(field, 'a'), ['ABBE', 'AIKE']);
});

test('exchange completions are empty when there are no valid values', () => {
  assert.deepEqual(exchangeCompletionMatches({}, 'a'), []);
});

test('exchange completions are limited', () => {
  const field = { valid_values: Array.from({ length: 120 }, (_, index) => `A${index}`) };
  assert.equal(exchangeCompletionMatches(field, 'a').length, 100);
  assert.equal(exchangeCompletionMatches(field, 'a', 7).length, 7);
});
