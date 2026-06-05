import assert from 'node:assert/strict';
import test from 'node:test';
import {
  availableModeOptions,
  callsignClearThresholdHz,
  callsignHasQuery,
  cwActionForMessage,
  shouldBlockEsmCallEnter,
  cwActionFromTemplate,
  cwActiveTimeoutMs,
  esmEnterAction,
  modeIsCw,
  nextCwWpm,
  normalizedContactFrequencyHz,
  typedModeFromCallsignInput,
} from './mainWindowHelpers.js';

test('availableModeOptions includes concrete selectable modes', () => {
  assert.deepEqual(availableModeOptions({ allowed_modes: ['cw'] }), [
    'CW',
    'CW-R',
    'SSB',
    'FM',
    'FT8',
    'JT65',
    'JT9',
    'MFSK',
    'PSK',
    'RTTY',
  ]);
});

test('typedModeFromCallsignInput matches exact mode tokens only', () => {
  const settings = { allowed_modes: ['cw', 'rtty', 'ssb'] };

  assert.equal(typedModeFromCallsignInput('cw', settings), 'CW');
  assert.equal(typedModeFromCallsignInput('cw-r', settings), 'CW-R');
  assert.equal(typedModeFromCallsignInput('ft8', settings), 'FT8');
  assert.equal(typedModeFromCallsignInput('RTTY', settings), 'RTTY');
  assert.equal(typedModeFromCallsignInput(' fm ', {}), 'FM');
  assert.equal(typedModeFromCallsignInput('AM', settings), null);
  assert.equal(typedModeFromCallsignInput('K1CW', settings), null);
  assert.equal(typedModeFromCallsignInput('ss', settings), null);
  assert.equal(typedModeFromCallsignInput('', settings), null);
});

test('modeIsCw treats CW-R as CW', () => {
  assert.equal(modeIsCw('CW'), true);
  assert.equal(modeIsCw('CW-R'), true);
  assert.equal(modeIsCw('RTTY'), false);
});

test('callsign clear threshold distinguishes phone modes', () => {
  assert.equal(callsignClearThresholdHz('CW'), 100);
  assert.equal(callsignClearThresholdHz('FT8'), 100);
  assert.equal(callsignClearThresholdHz('SSB'), 200);
  assert.equal(callsignClearThresholdHz('FM'), 200);
});

test('normalizedContactFrequencyHz accepts hertz and MHz values', () => {
  assert.equal(normalizedContactFrequencyHz(14074000), 14074000);
  assert.equal(normalizedContactFrequencyHz('14.074'), 14074000);
  assert.equal(normalizedContactFrequencyHz(''), 0);
});

test('callsignHasQuery detects incomplete queried callsigns', () => {
  assert.equal(callsignHasQuery('WB4?'), true);
  assert.equal(callsignHasQuery(' WB4? '), true);
  assert.equal(callsignHasQuery('K1ABC'), false);
  assert.equal(callsignHasQuery(''), false);
});

test('shouldBlockEsmCallEnter blocks only non-empty invalid callsigns', () => {
  assert.equal(shouldBlockEsmCallEnter('', false), false);
  assert.equal(shouldBlockEsmCallEnter('   ', false), false);
  assert.equal(shouldBlockEsmCallEnter('K1ABC', true), false);
  assert.equal(shouldBlockEsmCallEnter('WB4?', false), true);
  assert.equal(shouldBlockEsmCallEnter('KABC', false), true);
});

test('nextCwWpm clamps page-up and page-down changes to valid range', () => {
  assert.equal(nextCwWpm(20, 1), 21);
  assert.equal(nextCwWpm(20, -1), 19);
  assert.equal(nextCwWpm(60, 1), 60);
  assert.equal(nextCwWpm(5, -1), 5);
  assert.equal(nextCwWpm(Number.NaN, 1), 21);
});

test('cwActiveTimeoutMs waits for completion-capable keyers', () => {
  assert.equal(cwActiveTimeoutMs('winkeyer'), 30000);
  assert.equal(cwActiveTimeoutMs('cat'), 30000);
  assert.equal(cwActiveTimeoutMs('serial'), 30000);
  assert.equal(cwActiveTimeoutMs('none'), 500);
});

test('cwActionFromTemplate parses {Action:...} tokens only', () => {
  assert.equal(cwActionFromTemplate('{Action:Clear}'), 'Clear');
  assert.equal(cwActionFromTemplate(' { action : Clear } '), 'Clear');
  assert.equal(cwActionFromTemplate('CQ TEST'), null);
  assert.equal(cwActionFromTemplate('{CALL}'), null);
  assert.equal(cwActionFromTemplate('{Action:Clear} TU'), null);
});

test('cwActionForMessage returns action by mode and key', () => {
  const config = `
# RUN Messages
F1 Cq,CQ TEST
F12 Clear,{Action:Clear}
# S&P Messages
F12 Clear,CQ
`;

  assert.equal(cwActionForMessage(config, 'run', 'F12'), 'Clear');
  assert.equal(cwActionForMessage(config, 's&p', 'F12'), null);
  assert.equal(cwActionForMessage(config, 'run', 'F1'), null);
  assert.equal(cwActionForMessage(config, 'run', 'F9'), null);
});

test('esmEnterAction follows run mode matrix states', () => {
  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'Run',
      callsign: '',
      exchangeValid: false,
      exchangeSentCallsign: '',
      runCallsignAttempt: '',
    }),
    {
      keys: ['F1'],
      shouldLog: false,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: '',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'Run',
      callsign: 'K1ABC',
      exchangeValid: false,
      exchangeSentCallsign: '',
      runCallsignAttempt: '',
    }),
    {
      keys: ['F5', 'F2'],
      shouldLog: false,
      nextRunCallsignAttempt: 'K1ABC',
      nextExchangeSentCallsign: 'K1ABC',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'Run',
      callsign: 'K1ABC',
      exchangeValid: false,
      exchangeSentCallsign: '',
      runCallsignAttempt: 'K1ABC',
    }),
    {
      keys: ['F8'],
      shouldLog: false,
      nextRunCallsignAttempt: 'K1ABC',
      nextExchangeSentCallsign: '',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'Run',
      callsign: 'K1ABC',
      exchangeValid: true,
      exchangeSentCallsign: '',
      runCallsignAttempt: '',
    }),
    {
      keys: ['F5', 'F2'],
      shouldLog: false,
      nextRunCallsignAttempt: 'K1ABC',
      nextExchangeSentCallsign: 'K1ABC',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'Run',
      callsign: 'K1ABC',
      exchangeValid: true,
      exchangeSentCallsign: 'K1ABC',
      runCallsignAttempt: 'K1ABC',
    }),
    {
      keys: ['F3'],
      shouldLog: true,
      nextRunCallsignAttempt: 'K1ABC',
      nextExchangeSentCallsign: 'K1ABC',
    },
  );
});

test('esmEnterAction follows s&p mode matrix states', () => {
  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'S&P',
      callsign: '',
      exchangeValid: false,
      exchangeSentCallsign: '',
      runCallsignAttempt: '',
    }),
    {
      keys: ['F4'],
      shouldLog: false,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: '',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'S&P',
      callsign: 'K1ABC',
      exchangeValid: false,
      exchangeSentCallsign: '',
      runCallsignAttempt: '',
    }),
    {
      keys: ['F4'],
      shouldLog: false,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: '',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'S&P',
      callsign: 'K1ABC',
      exchangeValid: true,
      exchangeSentCallsign: '',
      runCallsignAttempt: '',
    }),
    {
      keys: ['F2'],
      shouldLog: true,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: 'K1ABC',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'S&P',
      callsign: 'K1ABC',
      exchangeValid: true,
      exchangeSentCallsign: 'K1ABC',
      runCallsignAttempt: '',
    }),
    {
      keys: [],
      shouldLog: true,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: 'K1ABC',
    },
  );
});

test('esmEnterAction returns no action when disabled', () => {
  assert.deepEqual(
    esmEnterAction({
      esmEnabled: false,
      operatingMode: 'Run',
      callsign: 'K1ABC',
      exchangeValid: true,
      exchangeSentCallsign: 'K1ABC',
      runCallsignAttempt: 'K1ABC',
    }),
    {
      keys: [],
      shouldLog: false,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: 'K1ABC',
    },
  );
});
