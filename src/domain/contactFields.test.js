import assert from 'node:assert/strict';
import test from 'node:test';
import {
  buildSentExchange,
  cutNumberString,
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
  assert.deepEqual(parseFieldType('RST', 'CW-R'), {
    kind: 'RST',
    maxLength: 3,
  });
  assert.deepEqual(parseFieldType('RST', 'SSB'), { kind: 'RST', maxLength: 2 });
});

test('sanitizeRST keeps valid RST digits for the active mode', () => {
  assert.equal(sanitizeRST('599', 'CW'), '599');
  assert.equal(sanitizeRST('599', 'CW-R'), '599');
  assert.equal(sanitizeRST('599', 'SSB'), '59');
  assert.equal(sanitizeRST('abc5799', 'CW'), '579');
  assert.equal(sanitizeRST('999', 'CW'), '');
});

test('sanitizeCallsign uppercases, filters chars, and truncates callsigns', () => {
  assert.equal(
    sanitizeCallsign('k1abcdefghijklmnopqrstuvwxyz'),
    'K1ABCDEFGHIJ',
  );
  assert.equal(sanitizeCallsign(' wb4? /x*'), 'WB4?/X');
  assert.equal(sanitizeCallsign('k 1 a b c'), 'K1ABC');
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

test('cutNumberString applies CW cut numbers for 9', () => {
  assert.equal(cutNumberString('599'), '5NN');
  assert.equal(cutNumberString(59), '5N');
});

test('buildSentExchange uses is_sent fields in order with cut RST and fixed params', () => {
  const settings = {
    exchange: [
      {
        name: 'RST(s)',
        type: 'RST',
        adif: 'RST_SENT',
        default: 599,
        is_sent: true,
      },
      {
        name: 'County',
        type: 'String:4',
        adif: 'STX_STRING',
        fixed: true,
        source_param: 'County',
        is_sent: true,
      },
      {
        name: 'Exchange',
        type: 'String:4',
        adif: 'SRX_STRING',
        is_sent: false,
      },
    ],
  };

  assert.equal(
    buildSentExchange(settings, {}, 'CW', { County: 'berk' }),
    '5NN BERK',
  );
  assert.equal(
    buildSentExchange(settings, { 'RST(s)': '579' }, 'CW', { County: 'berk' }),
    '57N BERK',
  );
});
