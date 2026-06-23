import assert from 'node:assert/strict';
import test from 'node:test';
import {
  availableModeOptions,
  callsignClearThresholdHz,
  callsignHasQuery,
  cwActionForMessage,
  messageActionForRadioMode,
  messageButtonIsSendable,
  shouldBlockEsmCallEnter,
  cwActionFromTemplate,
  cwActiveTimeoutMs,
  correctedEsmCallsignText,
  esmEnterAction,
  modeIsCw,
  nextCwWpm,
  isPageUpKey,
  isPageDownKey,
  previousContactExchangeAutofill,
  normalizedContactFrequencyHz,
  shouldAdvanceFromCallsignAutofill,
  tuningIncrementHzForMode,
  steppedFrequencyHz,
  typedModeFromCallsignInput,
} from './mainWindowHelpers.js';

test('availableModeOptions includes concrete selectable modes', () => {
  assert.deepEqual(availableModeOptions({ allowed_modes: ['cw'] }), [
    'CW',
    'CW-R',
    'SSB',
    'FM',
    'AM',
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
  assert.equal(typedModeFromCallsignInput('cwr', settings), 'CW-R');
  assert.equal(typedModeFromCallsignInput('ft8', settings), 'FT8');
  assert.equal(typedModeFromCallsignInput('RTTY', settings), 'RTTY');
  assert.equal(typedModeFromCallsignInput(' fm ', {}), 'FM');
  assert.equal(typedModeFromCallsignInput('AM', settings), 'AM');
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

test('page key helpers accept standard and legacy browser key values', () => {
  assert.equal(isPageUpKey({ key: 'PageUp' }), true);
  assert.equal(isPageUpKey({ key: 'Prior' }), true);
  assert.equal(isPageUpKey({ key: 'PageDown' }), false);

  assert.equal(isPageDownKey({ key: 'PageDown' }), true);
  assert.equal(isPageDownKey({ key: 'Next' }), true);
  assert.equal(isPageDownKey({ key: 'PageUp' }), false);
});

test('tuningIncrementHzForMode picks mode-specific configured values', () => {
  assert.equal(
    tuningIncrementHzForMode(
      { cw_tuning_increment_hz: 20, ssb_tuning_increment_hz: 100 },
      'CW',
    ),
    20,
  );
  assert.equal(
    tuningIncrementHzForMode(
      { cw_tuning_increment_hz: 20, ssb_tuning_increment_hz: 125 },
      'SSB',
    ),
    125,
  );
  assert.equal(tuningIncrementHzForMode({}, 'CW-R'), 20);
  assert.equal(tuningIncrementHzForMode({}, 'FT8'), 100);
});

test('steppedFrequencyHz clamps values at 1 Hz minimum', () => {
  assert.equal(steppedFrequencyHz(7000000, 100), 7000100);
  assert.equal(steppedFrequencyHz(20, -100), 1);
});

test('cwActiveTimeoutMs waits for completion-capable keyers', () => {
  assert.equal(cwActiveTimeoutMs('winkeyer'), 30000);
  assert.equal(cwActiveTimeoutMs('cat'), 30000);
  assert.equal(cwActiveTimeoutMs('serial'), 30000);
  assert.equal(cwActiveTimeoutMs('none'), 500);
});

test('shouldAdvanceFromCallsignAutofill advances only when ESM and editable exchange fields exist', () => {
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: true,
      autofillResult: { matchedContact: { CALL: 'K1ABC' } },
      hasEditableExchangeField: true,
    }),
    true,
  );
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: true,
      autofillResult: { matchedContact: null },
      hasEditableExchangeField: true,
    }),
    false,
  );
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: false,
      autofillResult: { matchedContact: { CALL: 'K1ABC' } },
      hasEditableExchangeField: true,
    }),
    false,
  );
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: true,
      autofillResult: { matchedContact: { CALL: 'K1ABC' } },
      hasEditableExchangeField: false,
    }),
    false,
  );
});

test('cwActionFromTemplate parses {Action:...} tokens only', () => {
  assert.equal(cwActionFromTemplate('{Action:Clear}'), 'Clear');
  assert.equal(cwActionFromTemplate(' { action : Clear } '), 'Clear');
  assert.equal(cwActionFromTemplate('CQ TEST'), null);
  assert.equal(cwActionFromTemplate('{CALL}'), null);
  assert.equal(cwActionFromTemplate('{Action:Clear} TU'), null);
});

test('messageButtonIsSendable requires a non-empty message label', () => {
  assert.equal(messageButtonIsSendable({ key: 'F11', label: '-' }), false);
  assert.equal(messageButtonIsSendable({ key: 'F12', label: '-' }), false);
  assert.equal(messageButtonIsSendable({ key: 'F1', label: 'Cq' }), true);
});

test('messageActionForRadioMode uses CW config in CW modes and voice config in phone modes', () => {
  const cwConfig = `
# RUN Messages
F12 Clear,{Action:Clear}
# S&P Messages
F12 Clear,{Action:Clear}
`;
  const voiceConfig = `
# RUN Messages
F12 Voice Clear,{Action:Clear}
# S&P Messages
F12 Voice Clear,{Action:Clear}
`;

  assert.equal(
    messageActionForRadioMode(cwConfig, voiceConfig, 'run', 'F12', 'CW'),
    'Clear',
  );
  assert.equal(
    messageActionForRadioMode(cwConfig, voiceConfig, 'run', 'F12', 'SSB'),
    'Clear',
  );
  assert.equal(
    messageActionForRadioMode(cwConfig, voiceConfig, 's&p', 'F12', 'FM'),
    'Clear',
  );
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

test('correctedEsmCallsignText returns suffix-only or full callsign corrections', () => {
  assert.equal(correctedEsmCallsignText('KB1AWN', 'KB1AWM'), 'AWM');
  assert.equal(correctedEsmCallsignText('KD1AWM', 'KB1AWM'), 'KB1AWM');
  assert.equal(correctedEsmCallsignText('3DA0RU', '3DA0RW'), 'RW');
  assert.equal(correctedEsmCallsignText('K1ABC', 'K1ABC'), '');
});

test('previousContactExchangeAutofill copies non-serial fields from exact callsign match', () => {
  const settings = {
    exchange: [
      { name: 'Serial', type: 'Serial:4', adif: 'STX', is_sent: true },
      { name: 'Name', type: 'String:10', adif: 'NAME' },
      { name: 'QTH', type: 'String:5', adif: 'QTH' },
    ],
  };
  const newestContact = {
    CALL: 'K1ABC',
    STX: '123',
    NAME: 'alice',
    QTH: 'ny',
  };

  const result = previousContactExchangeAutofill({
    settings,
    contacts: [
      newestContact,
      { CALL: 'K1ABC', STX: '122', NAME: 'older', QTH: 'ma' },
    ],
    callsign: ' k1abc ',
    exchangeValues: { Serial: '999', Name: '', QTH: '' },
    radioMode: 'CW',
  });

  assert.equal(result.matchedContact, newestContact);
  assert.equal(result.changed, true);
  assert.deepEqual(result.copiedFields, ['Name', 'QTH']);
  assert.deepEqual(result.values, {
    Serial: '999',
    Name: 'ALICE',
    QTH: 'NY',
  });
});

test('previousContactExchangeAutofill preserves user-entered values and requires exact callsign', () => {
  const settings = {
    exchange: [
      { name: 'Name', type: 'String:10', adif: 'NAME' },
      { name: 'Section', type: 'String:3', adif: 'ARRL_SECT' },
    ],
  };

  const prefixOnly = previousContactExchangeAutofill({
    settings,
    contacts: [{ CALL: 'K1ABC', NAME: 'Alice', ARRL_SECT: 'SC' }],
    callsign: 'K1A',
    exchangeValues: { Name: '', Section: '' },
  });

  assert.equal(prefixOnly.matchedContact, null);
  assert.equal(prefixOnly.changed, false);
  assert.deepEqual(prefixOnly.values, { Name: '', Section: '' });

  const exact = previousContactExchangeAutofill({
    settings,
    contacts: [{ Call: 'k1abc', NAME: 'Alice', ARRL_SECT: 'SC' }],
    callsign: 'K1ABC',
    exchangeValues: { Name: 'BOB', Section: '' },
  });

  assert.equal(exact.changed, true);
  assert.deepEqual(exact.copiedFields, ['Section']);
  assert.deepEqual(exact.values, { Name: 'BOB', Section: 'SC' });
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
      correctionText: '',
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
      correctionText: '',
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
      correctionText: '',
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
      correctionText: '',
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
      correctionText: '',
      shouldLog: true,
      nextRunCallsignAttempt: 'K1ABC',
      nextExchangeSentCallsign: 'K1ABC',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'Run',
      callsign: 'KB1AWM',
      exchangeValid: true,
      exchangeSentCallsign: 'KB1AWN',
      runCallsignAttempt: 'KB1AWN',
    }),
    {
      keys: ['F3'],
      correctionText: 'AWM',
      shouldLog: true,
      nextRunCallsignAttempt: 'KB1AWM',
      nextExchangeSentCallsign: 'KB1AWM',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'Run',
      callsign: 'KB1AWM',
      exchangeValid: true,
      exchangeSentCallsign: 'KD1AWM',
      runCallsignAttempt: 'KD1AWM',
    }),
    {
      keys: ['F3'],
      correctionText: 'KB1AWM',
      shouldLog: true,
      nextRunCallsignAttempt: 'KB1AWM',
      nextExchangeSentCallsign: 'KB1AWM',
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
      correctionText: '',
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
      correctionText: '',
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
      correctionText: '',
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
      correctionText: '',
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
      correctionText: '',
      shouldLog: false,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: 'K1ABC',
    },
  );
});
