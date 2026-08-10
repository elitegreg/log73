import assert from 'node:assert/strict';
import test from 'node:test';
import {
  fieldValueLabel,
  validateCallsign,
  validateConfiguredField,
  validateExchangeField,
} from './validation.js';

test('fieldValueLabel uses label, key, id, then Field fallback', () => {
  assert.equal(fieldValueLabel({ label: 'Section', key: 'Sect' }), 'Section');
  assert.equal(fieldValueLabel({ key: 'Sect' }), 'Sect');
  assert.equal(fieldValueLabel({ id: 'section' }), 'section');
  assert.equal(fieldValueLabel({}), 'Field');
});

test('validateCallsign enforces logging rules', () => {
  assert.equal(validateCallsign('K1ABC').ok, true);
  assert.equal(validateCallsign('4O9A').ok, true);
  assert.equal(validateCallsign('KA1BC').ok, true);
  assert.equal(validateCallsign('K1ABC/4').ok, true);
  assert.equal(validateCallsign('K1ABC/P').ok, true);

  assert.equal(validateCallsign('').ok, false);
  assert.equal(validateCallsign('WB4?').ok, false);
  assert.equal(validateCallsign('K 1ABC').ok, false);
  assert.equal(validateCallsign('K1A*').ok, false);
  assert.equal(validateCallsign('/K1ABC').ok, false);
  assert.equal(validateCallsign('K1ABC/').ok, false);
  assert.equal(validateCallsign('K1/A/BC').ok, false);
  assert.equal(validateCallsign('KABC').ok, false);
});

test('validateExchangeField requires non-empty values', () => {
  const result = validateExchangeField(
    { label: 'Section', input: { kind: 'string', max_length: 3 } },
    '',
  );
  assert.equal(result.ok, false);
  assert.match(result.error, /Section is required/);
});

test('validateExchangeField conditionally requires or forbids an exchange', () => {
  const field = {
    label: 'Section',
    input: { kind: 'string', max_length: 3 },
    validation: { values: ['EMA', 'ONN'] },
    only_when: { field: 'DXCC', values: ['1', '291'] },
  };
  assert.equal(validateExchangeField(field, '', 'CW', { DXCC: 291 }).ok, false);
  assert.equal(
    validateExchangeField(field, 'ema', 'CW', { DXCC: 291 }).ok,
    true,
  );
  assert.equal(validateExchangeField(field, '', 'CW', { DXCC: 230 }).ok, true);
  assert.equal(
    validateExchangeField(field, 'EMA', 'CW', { DXCC: 230 }).ok,
    false,
  );
});

test('validateExchangeField validates RST by mode', () => {
  assert.equal(
    validateExchangeField({ label: 'RST', input: { kind: 'rst' } }, '599', 'CW')
      .ok,
    true,
  );
  assert.equal(
    validateExchangeField(
      { label: 'RST', input: { kind: 'rst' } },
      '599',
      'CW-R',
    ).ok,
    true,
  );
  assert.equal(
    validateExchangeField({ label: 'RST', input: { kind: 'rst' } }, '59', 'CW')
      .ok,
    false,
  );
  assert.equal(
    validateExchangeField({ label: 'RST', input: { kind: 'rst' } }, '59', 'SSB')
      .ok,
    true,
  );
});

test('validateExchangeField validates numeric fields', () => {
  assert.equal(
    validateExchangeField(
      { label: 'Serial', input: { kind: 'numeric', max_length: 3 } },
      '123',
    ).ok,
    true,
  );
  assert.equal(
    validateExchangeField(
      { label: 'Serial', input: { kind: 'serial', max_length: 3 } },
      '123',
    ).ok,
    true,
  );
  assert.equal(
    validateExchangeField(
      { label: 'Serial', input: { kind: 'numeric', max_length: 3 } },
      '12A',
    ).ok,
    false,
  );
  assert.equal(
    validateExchangeField(
      { label: 'Serial', input: { kind: 'serial', max_length: 3 } },
      '12A',
    ).ok,
    false,
  );
});

test('validateExchangeField validates configured values case-insensitively', () => {
  const field = {
    label: 'State',
    input: { kind: 'string', max_length: 4 },
    validation: { values: ['SC', 'NC'] },
  };
  assert.equal(validateExchangeField(field, 'sc').ok, true);
  assert.equal(validateExchangeField(field, 'GA').ok, false);
});

test('validateConfiguredField accepts configured values or an alternate regex', () => {
  const field = {
    label: 'Location',
    input: { kind: 'string', max_length: 4 },
    validation: {
      values: ['CT', 'CMX', '1', '2', '3'],
      pattern: '^\\d{1,4}$',
      match_mode: 'any',
    },
  };
  assert.equal(validateConfiguredField(field, 'cmx').ok, true);
  assert.equal(validateConfiguredField(field, '1234').ok, true);
  assert.equal(validateConfiguredField(field, 'ZZ').ok, false);
  assert.equal(validateConfiguredField(field, '12345').ok, false);
});

test('validateExchangeField accepts TN and MDC locations without spaces', () => {
  const tnLocation = {
    label: 'Location',
    input: { kind: 'string', max_length: 4 },
    validation: {
      values: ['ANDE', 'SC', 'SK'],
      pattern: '^\\S+$',
      match_mode: 'any',
    },
  };
  const mdcLocation = {
    label: 'Location',
    input: { kind: 'string', max_length: 16 },
    validation: {
      values: ['BAL', 'SC', 'SK'],
      pattern: '^\\S+$',
      match_mode: 'any',
    },
  };

  for (const field of [tnLocation, mdcLocation]) {
    assert.equal(
      validateExchangeField(field, field.validation.values[0]).ok,
      true,
    );
    assert.equal(validateExchangeField(field, 'DL').ok, true);
    assert.equal(validateExchangeField(field, 'D L').ok, false);
  }
});

test('validateExchangeField validates configured values when a field has in_sets', () => {
  const field = {
    label: 'Location',
    input: { kind: 'string', max_length: 16 },
    validation: { values: ['SC', 'NC'] },
  };
  assert.equal(validateExchangeField(field, 'sc').ok, true);
  assert.equal(validateExchangeField(field, 'Somewhere').ok, false);
});

test('validateExchangeField validates regex patterns', () => {
  const field = {
    label: 'Class',
    input: { kind: 'string', max_length: 3 },
    validation: { pattern: '^\\d+[A-F]$' },
  };
  assert.equal(validateExchangeField(field, '1A').ok, true);
  assert.equal(validateExchangeField(field, 'ABC').ok, false);
});

test('validateExchangeField reports invalid regex patterns', () => {
  const result = validateExchangeField(
    {
      label: 'Field',
      input: { kind: 'string', max_length: 3 },
      validation: { pattern: '[' },
    },
    'ABC',
  );
  assert.equal(result.ok, false);
  assert.match(result.error, /invalid validation pattern/);
});

test('validateConfiguredField supports optional and multiline fields', () => {
  assert.equal(
    validateConfiguredField(
      {
        label: 'Soapbox',
        input: { kind: 'string', max_length: 75 },
        widget: 'textarea',
        validation: { required: false },
        max_lines: 2,
        preserve_case: true,
      },
      '',
    ).ok,
    true,
  );

  const tooManyLines = validateConfiguredField(
    {
      label: 'Address',
      input: { kind: 'string', max_length: 45 },
      widget: 'textarea',
      max_lines: 2,
      preserve_case: true,
    },
    'Line 1\nLine 2\nLine 3',
  );
  assert.equal(tooManyLines.ok, false);
  assert.match(tooManyLines.error, /at most 2 lines/);
});
