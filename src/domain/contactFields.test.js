import assert from 'node:assert/strict';
import test from 'node:test';
import {
  buildSentExchange,
  cutNumberString,
  fieldDefault,
  parseFieldInput,
  sanitizeCallsign,
  sanitizeConfiguredValue,
  sanitizeExchangeValue,
  sanitizeRST,
} from './contactFields.js';

test('parseFieldInput uses contest lengths and RST mode lengths', () => {
  assert.deepEqual(parseFieldInput({ kind: 'string', max_length: 4 }), {
    kind: 'STRING',
    maxLength: 4,
  });
  assert.deepEqual(parseFieldInput({ kind: 'numeric', max_length: 3 }), {
    kind: 'NUMERIC',
    maxLength: 3,
  });
  assert.deepEqual(parseFieldInput({ kind: 'serial', max_length: 4 }), {
    kind: 'SERIAL',
    maxLength: 4,
  });
  assert.deepEqual(parseFieldInput({ kind: 'rst' }, 'CW'), {
    kind: 'RST',
    maxLength: 3,
  });
  assert.deepEqual(parseFieldInput({ kind: 'rst' }, 'CW-R'), {
    kind: 'RST',
    maxLength: 3,
  });
  assert.deepEqual(parseFieldInput({ kind: 'rst' }, 'SSB'), {
    kind: 'RST',
    maxLength: 2,
  });
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
  assert.equal(
    sanitizeExchangeValue(
      { input: { kind: 'numeric', max_length: 3 } },
      '123A',
    ),
    '123',
  );
  assert.equal(
    sanitizeExchangeValue({ input: { kind: 'serial', max_length: 4 } }, '001A'),
    '001',
  );
  assert.equal(
    sanitizeExchangeValue({ input: { kind: 'string', max_length: 4 } }, 'scqp'),
    'SCQP',
  );
  assert.equal(
    sanitizeExchangeValue({ input: { kind: 'rst' } }, '599', 'SSB'),
    '59',
  );
});

test('sanitizeConfiguredValue preserves case and line structure for textarea fields', () => {
  assert.equal(
    sanitizeConfiguredValue(
      {
        input: { kind: 'string', max_length: 5 },
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
    fieldDefault(
      { input: { kind: 'string', max_length: 4 }, source: 'County' },
      'CW',
      {
        County: 'abbe',
      },
    ),
    'ABBE',
  );
  assert.equal(
    fieldDefault({ input: { kind: 'rst' }, default: 599 }, 'SSB'),
    '59',
  );
  assert.equal(
    fieldDefault({ input: { kind: 'string', max_length: 4 } }, 'CW'),
    '',
  );
});

test('cutNumberString applies CW cut numbers for 9', () => {
  assert.equal(cutNumberString('599'), '5NN');
  assert.equal(cutNumberString(59), '5N');
});

test('buildSentExchange uses sent fields in order with cut RST and fixed params', () => {
  const settings = {
    exchange: [
      {
        id: 'rst-sent',
        label: 'RST(s)',
        input: { kind: 'rst' },
        adif: 'RST_SENT',
        default: 599,
        direction: 'sent',
      },
      {
        id: 'county-sent',
        label: 'County',
        input: { kind: 'string', max_length: 4 },
        adif: 'STX_STRING',
        fixed: true,
        source: 'County',
        direction: 'sent',
      },
      {
        id: 'exchange-received',
        label: 'Exchange',
        input: { kind: 'string', max_length: 4 },
        adif: 'SRX_STRING',
        direction: 'received',
      },
    ],
  };

  assert.equal(
    buildSentExchange(settings, {}, 'CW', { County: 'berk' }),
    '5NN BERK',
  );
  assert.equal(
    buildSentExchange(settings, { 'rst-sent': '579' }, 'CW', {
      County: 'berk',
    }),
    '57N BERK',
  );
});
