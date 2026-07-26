import assert from 'node:assert/strict';
import test from 'node:test';
import {
  cabrilloTransmitterAdif,
  cabrilloTransmitterPrompt,
} from './cabrilloTransmitter.js';

function settings({ flagged = false } = {}) {
  return {
    cabrillo: {
      log_fields: [
        {
          name: 'CATEGORY-OPERATOR',
          default: 'SINGLE-OP',
        },
        {
          name: 'CATEGORY-TRANSMITTER',
          default: 'ONE',
          multi_single_has_mult_transmitter: flagged,
        },
      ],
    },
  };
}

function log(operator, transmitter) {
  return {
    contest_params: {
      'CATEGORY-OPERATOR': operator,
      'CATEGORY-TRANSMITTER': transmitter,
    },
  };
}

test('multi-two prompt asks for transmitter ID and maps One and Two to 0 and 1', () => {
  assert.deepEqual(
    cabrilloTransmitterPrompt(settings(), log(' multi-op ', ' two ')),
    {
      kind: 'multi-two',
      question: 'Transmitter ID?',
      options: [
        { id: 0, label: 'One' },
        { id: 1, label: 'Two' },
      ],
    },
  );
});

test('flagged multi-single prompt maps run and mults transmitters to 0 and 1', () => {
  assert.deepEqual(
    cabrilloTransmitterPrompt(
      settings({ flagged: true }),
      log('MULTI-OP', 'ONE'),
    ),
    {
      kind: 'multi-single',
      question: null,
      options: [
        { id: 0, label: 'Run Transmitter' },
        { id: 1, label: 'Mults Transmitter' },
      ],
    },
  );
});

test('unflagged multi-single and ineligible categories do not prompt', () => {
  assert.equal(
    cabrilloTransmitterPrompt(settings(), log('MULTI-OP', 'ONE')),
    null,
  );
  assert.equal(
    cabrilloTransmitterPrompt(
      settings({ flagged: true }),
      log('SINGLE-OP', 'TWO'),
    ),
    null,
  );
  assert.equal(
    cabrilloTransmitterPrompt(
      settings({ flagged: true }),
      log('MULTI-OP', 'UNLIMITED'),
    ),
    null,
  );
  assert.equal(cabrilloTransmitterPrompt({}, {}), null);
});

test('category defaults and fixed fields are used when log values are absent', () => {
  const fixedSettings = settings();
  fixedSettings.cabrillo.fixed_fields = [
    { name: 'CATEGORY-OPERATOR', value: 'MULTI-OP' },
    { name: 'CATEGORY-TRANSMITTER', value: 'TWO' },
  ];

  assert.equal(
    cabrilloTransmitterPrompt(fixedSettings, { contest_params: {} })?.kind,
    'multi-two',
  );
  assert.equal(
    cabrilloTransmitterPrompt(settings(), { contest_params: {} }),
    null,
  );
});

test('ADIF transmitter field is emitted only for IDs 0 and 1', () => {
  assert.deepEqual(cabrilloTransmitterAdif(0), { APP_LOG73_TX_ID: 0 });
  assert.deepEqual(cabrilloTransmitterAdif(1), { APP_LOG73_TX_ID: 1 });
  assert.deepEqual(cabrilloTransmitterAdif(null), {});
  assert.deepEqual(cabrilloTransmitterAdif(2), {});
});
