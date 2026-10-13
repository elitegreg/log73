import assert from 'node:assert/strict';
import test from 'node:test';
import {
  loadCallsignFixture,
  loadConditionsFixture,
} from '../../tests/fixtures/domain/loadFixture.js';
import { conditionApplies } from './conditions.js';
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

test('validateCallsign accepts portable prefixes and enforces length', () => {
  for (const input of ['DL/W1AW', 'F/W1AW', 'PA/W1AW', 'EA/W1AW', 'G/W1AW']) {
    assert.equal(validateCallsign(input).ok, true, input);
  }
  assert.equal(validateCallsign('DL/W1AW12345').ok, true);
  assert.equal(validateCallsign('DL/W1AW123456').ok, false);
});

test('validateCallsign query mode tolerates wildcards', () => {
  assert.equal(validateCallsign('W1?', true).ok, true);
  assert.equal(validateCallsign('K?ABC', true).ok, true);
  assert.equal(validateCallsign('W1ABC', true).ok, true);
  assert.equal(validateCallsign('K1/A/BC', true).ok, false);
  assert.equal(validateCallsign('KABC', true).ok, false);
});

test('shared fixture logging and query validity match', () => {
  const fixture = loadCallsignFixture();
  assert.equal(fixture.version, 1);
  for (const kase of fixture.cases) {
    assert.equal(
      validateCallsign(kase.input).ok,
      kase.logging_valid,
      `logging validity for ${kase.input}`,
    );
    assert.equal(
      validateCallsign(kase.input, true).ok,
      kase.query_valid,
      `query validity for ${kase.input}`,
    );
  }
});

test('validateExchangeField requires non-empty values', () => {
  const result = validateExchangeField(
    { label: 'Section', input: { kind: 'string', max_length: 3 } },
    '',
  );
  assert.equal(result.ok, false);
  assert.match(result.error, /Section is required/);
});

test('shared conditions fixture matches', () => {
  const fixture = loadConditionsFixture();
  assert.equal(fixture.version, 1);
  assert.ok(fixture.cases.length > 0);
  for (const kase of fixture.cases) {
    assert.equal(
      conditionApplies(kase.condition, kase.fields, kase.meta),
      kase.applies,
      kase.name,
    );
  }
});

test('validateExchangeField honors exclusions and field comparisons', () => {
  const serialField = {
    label: 'Serial',
    input: { kind: 'serial', max_length: 4 },
    only_when: { field: 'MY_DXCC', exclude_values: ['1'] },
  };
  // Inside Canada the field must stay blank.
  assert.equal(
    validateExchangeField(serialField, '', 'CW', { MY_DXCC: 1 }).ok,
    true,
  );
  assert.equal(
    validateExchangeField(serialField, '123', 'CW', { MY_DXCC: 1 }).ok,
    false,
  );
  // Outside Canada normal serial validation applies.
  assert.equal(
    validateExchangeField(serialField, '123', 'CW', { MY_DXCC: 291 }).ok,
    true,
  );

  const matchedField = {
    label: 'Match',
    input: { kind: 'string', max_length: 8 },
    only_when: { field: 'A', matches_field: 'B' },
  };
  assert.equal(
    validateExchangeField(matchedField, 'x', 'CW', { A: 'x', B: 'X' }).ok,
    true,
  );
  assert.equal(
    validateExchangeField(matchedField, 'x', 'CW', { A: 'x', B: 'y' }).ok,
    false,
  );
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
