import assert from 'node:assert/strict';
import test from 'node:test';
import {
  fieldDefault,
  parseFieldType,
  sanitizeCallsign,
  sanitizeConfiguredValue,
  sanitizeExchangeValue,
  sanitizeRST,
} from './contactFields.js';

test('parseFieldType uses contest lengths and RST mode lengths', () => {
  assert.deepEqual(parseFieldType('String:4'), {
    kind: 'STRING',
    maxLength: 4,
  });
  assert.deepEqual(parseFieldType('Numeric:3'), {
    kind: 'NUMERIC',
    maxLength: 3,
  });
  assert.deepEqual(parseFieldType('RST', 'CW'), { kind: 'RST', maxLength: 3 });
  assert.deepEqual(parseFieldType('RST', 'SSB'), { kind: 'RST', maxLength: 2 });
});

test('sanitizeRST keeps valid RST digits for the active mode', () => {
  assert.equal(sanitizeRST('599', 'CW'), '599');
  assert.equal(sanitizeRST('599', 'SSB'), '59');
  assert.equal(sanitizeRST('abc5799', 'CW'), '579');
  assert.equal(sanitizeRST('999', 'CW'), '');
});

test('sanitizeCallsign uppercases and truncates callsigns', () => {
  assert.equal(
    sanitizeCallsign('k1abcdefghijklmnopqrstuvwxyz'),
    'K1ABCDEFGHIJ',
  );
});

test('sanitizeExchangeValue applies type-specific normalization', () => {
  assert.equal(sanitizeExchangeValue({ type: 'Numeric:3' }, '123A'), '123');
  assert.equal(sanitizeExchangeValue({ type: 'String:4' }, 'scqp'), 'SCQP');
  assert.equal(sanitizeExchangeValue({ type: 'RST' }, '599', 'SSB'), '59');
});

test('sanitizeConfiguredValue preserves case and line structure for textarea fields', () => {
  assert.equal(
    sanitizeConfiguredValue(
      {
        type: 'String:5',
        widget: 'textarea',
        preserve_case: true,
        max_lines: 2,
      },
      'Alpha\nBravo\nCharlie',
    ),
    'Alpha\nBravo',
  );
});

test('fieldDefault reads source params and sanitizes RST defaults', () => {
  assert.equal(
    fieldDefault({ type: 'String:4', source_param: 'County' }, 'CW', {
      County: 'abbe',
    }),
    'ABBE',
  );
  assert.equal(fieldDefault({ type: 'RST', default: 599 }, 'SSB'), '59');
  assert.equal(fieldDefault({ type: 'String:4' }, 'CW'), '');
});
