import assert from 'node:assert/strict';
import test from 'node:test';
import { fieldValueLabel, validateExchangeField } from './validation.js';

test('fieldValueLabel uses label, name, then Field fallback', () => {
  assert.equal(fieldValueLabel({ label: 'Section', name: 'Sect' }), 'Section');
  assert.equal(fieldValueLabel({ name: 'Sect' }), 'Sect');
  assert.equal(fieldValueLabel({}), 'Field');
});

test('validateExchangeField requires non-empty values', () => {
  const result = validateExchangeField(
    { label: 'Section', type: 'String:3' },
    '',
  );
  assert.equal(result.ok, false);
  assert.match(result.error, /Section is required/);
});

test('validateExchangeField validates RST by mode', () => {
  assert.equal(
    validateExchangeField({ name: 'RST', type: 'RST' }, '599', 'CW').ok,
    true,
  );
  assert.equal(
    validateExchangeField({ name: 'RST', type: 'RST' }, '59', 'CW').ok,
    false,
  );
  assert.equal(
    validateExchangeField({ name: 'RST', type: 'RST' }, '59', 'SSB').ok,
    true,
  );
});

test('validateExchangeField validates numeric fields', () => {
  assert.equal(
    validateExchangeField({ name: 'Serial', type: 'Numeric:3' }, '123').ok,
    true,
  );
  assert.equal(
    validateExchangeField({ name: 'Serial', type: 'Numeric:3' }, '12A').ok,
    false,
  );
});

test('validateExchangeField validates configured values case-insensitively', () => {
  const field = { name: 'State', type: 'String:4', valid_values: ['SC', 'NC'] };
  assert.equal(validateExchangeField(field, 'sc').ok, true);
  assert.equal(validateExchangeField(field, 'GA').ok, false);
});

test('validateExchangeField validates regex patterns', () => {
  const field = { name: 'Class', type: 'String:3', regex: '^\\d+[A-F]$' };
  assert.equal(validateExchangeField(field, '1A').ok, true);
  assert.equal(validateExchangeField(field, 'ABC').ok, false);
});

test('validateExchangeField reports invalid regex patterns', () => {
  const result = validateExchangeField(
    { name: 'Field', type: 'String:3', regex: '[' },
    'ABC',
  );
  assert.equal(result.ok, false);
  assert.match(result.error, /invalid validation pattern/);
});
