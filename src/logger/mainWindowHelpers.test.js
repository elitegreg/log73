import assert from 'node:assert/strict';
import test from 'node:test';
import {
  availableModeOptions,
  bandByName,
  bandNamesEqual,
  bandForFrequency,
  catIndicatorState,
  callsignClearThresholdHz,
  loggerFrequencyChangeAction,
  callsignHasQuery,
  cwActionForMessage,
  messageActionForRadioMode,
  messageButtonIsSendable,
  shouldBlockEsmCallEnter,
  cwActionFromTemplate,
  cwActiveTimeoutMs,
  correctedEsmCallsignText,
  exchangeDefaults,
  esmEnterAction,
  esmStateAfterCallsignEdit,
  modeIsCw,
  nextCwWpm,
  isPageUpKey,
  isPageDownKey,
  previousContactExchangeAutofill,
  normalizedContactFrequencyHz,
  shouldAdvanceFromCallsignAutofill,
  spaceDelimitedWordAt,
  tuningIncrementHzForMode,
  steppedFrequencyHz,
  typedModeFromCallsignInput,
} from './mainWindowHelpers.js';

test('spaceDelimitedWordAt returns only the word under the offset', () => {
  const text = 'CQ K1ABC 599';

  assert.equal(spaceDelimitedWordAt(text, 0), 'CQ');
  assert.equal(spaceDelimitedWordAt(text, 4), 'K1ABC');
  assert.equal(spaceDelimitedWordAt(text, text.length - 1), '599');
  assert.equal(spaceDelimitedWordAt(text, 2), '');
  assert.equal(spaceDelimitedWordAt(text, text.length), '');
});

test('CAT indicator combines the radio websocket and CAT connection states', () => {
  assert.equal(catIndicatorState('connected', 'online'), 'connected');
  assert.equal(catIndicatorState('connected', 'offline'), 'degraded');
  assert.equal(catIndicatorState('disconnected', 'online'), 'disconnected');
});

test('exchangeDefaults uses the allocated sent serial and leaves received serial blank', () => {
  const settings = {
    exchange: [
      {
        id: 'serial-sent',
        label: 'Serial(s)',
        input: { kind: 'serial', max_length: 4 },
        adif: 'STX',
        direction: 'sent',
      },
      {
        id: 'serial-received',
        label: 'Serial',
        input: { kind: 'serial', max_length: 4 },
        adif: 'SRX',
        direction: 'received',
      },
    ],
  };

  assert.deepEqual(
    exchangeDefaults(
      settings,
      'CW',
      {},
      {
        required: true,
        fieldAdif: 'stx',
        current: 12,
      },
    ),
    {
      'serial-sent': '12',
      'serial-received': '',
    },
  );
  assert.deepEqual(
    exchangeDefaults(
      settings,
      'CW',
      {},
      {
        required: true,
        fieldAdif: 'STX',
        current: null,
      },
    ),
    {
      'serial-sent': '',
      'serial-received': '',
    },
  );
});

test('availableModeOptions prefers backend-provided mode catalog', () => {
  assert.deepEqual(availableModeOptions({ mode_catalog: ['CW', 'RTTY'] }), [
    'CW',
    'RTTY',
  ]);
  assert.deepEqual(availableModeOptions({ modes: ['CW'] }), [
    'CW',
    'CW-R',
    'SSB',
    'FM',
    'AM',
    'DATA',
    'RTTY',
  ]);
});

test('band helpers use backend-provided band catalog', () => {
  const bands = [
    { name: '40M', lowerHz: 7000000, upperHz: 7300000 },
    { name: '20M', lowerHz: 14000000, upperHz: 14350000 },
  ];

  assert.equal(bandForFrequency(14074000, bands)?.name, '20M');
  assert.equal(bandByName(bands, '40m')?.lowerHz, 7000000);
  assert.equal(bandByName(bands, ' 20m ')?.upperHz, 14350000);
  assert.equal(bandNamesEqual('23cm', '23CM'), true);
  assert.equal(bandNamesEqual('20m', '40m'), false);
  assert.equal(bandByName(bands, '15m'), undefined);
});

test('typedModeFromCallsignInput matches exact mode tokens only', () => {
  const settings = {
    mode_catalog: ['CW', 'CW-R', 'DATA', 'RTTY', 'SSB', 'FM', 'AM'],
  };

  assert.equal(typedModeFromCallsignInput('cw', settings), 'CW');
  assert.equal(typedModeFromCallsignInput('cw-r', settings), 'CW-R');
  assert.equal(typedModeFromCallsignInput('cwr', settings), 'CW-R');
  assert.equal(typedModeFromCallsignInput('data', settings), 'DATA');
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
  assert.equal(callsignClearThresholdHz('DATA'), 100);
  assert.equal(callsignClearThresholdHz('SSB'), 200);
  assert.equal(callsignClearThresholdHz('FM'), 200);
});

test('loggerFrequencyChangeAction clears on significant tuning changes and ignores pending band map tuning', () => {
  assert.equal(
    loggerFrequencyChangeAction({
      previousFrequencyHz: 14000000,
      nextFrequencyHz: 14000100,
      thresholdHz: 100,
    }),
    'clear-logger',
  );
  assert.equal(
    loggerFrequencyChangeAction({
      previousFrequencyHz: 14000000,
      nextFrequencyHz: 14000099,
      thresholdHz: 100,
    }),
    'none',
  );
  assert.equal(
    loggerFrequencyChangeAction({
      previousFrequencyHz: 14000000,
      nextFrequencyHz: 14000200,
      thresholdHz: 100,
      pendingBandMapTuneFrequencyHz: 14000250,
    }),
    'clear-pending-bandmap-tune',
  );
  assert.equal(
    loggerFrequencyChangeAction({
      previousFrequencyHz: null,
      nextFrequencyHz: 14000100,
      thresholdHz: 100,
    }),
    'none',
  );
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
  assert.equal(tuningIncrementHzForMode({}, 'DATA'), 100);
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

test('shouldAdvanceFromCallsignAutofill skips run mode so ESM sends the full sequence first', () => {
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: true,
      operatingMode: 'S&P',
      autofillResult: { matchedContact: { CALL: 'K1ABC' } },
      hasEditableExchangeField: true,
    }),
    true,
  );
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: true,
      operatingMode: 'Run',
      autofillResult: { matchedContact: { CALL: 'K1ABC' } },
      hasEditableExchangeField: true,
    }),
    false,
  );
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: true,
      operatingMode: 'S&P',
      autofillResult: { matchedContact: null },
      hasEditableExchangeField: true,
    }),
    false,
  );
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: false,
      operatingMode: 'S&P',
      autofillResult: { matchedContact: { CALL: 'K1ABC' } },
      hasEditableExchangeField: true,
    }),
    false,
  );
  assert.equal(
    shouldAdvanceFromCallsignAutofill({
      esmEnabled: true,
      operatingMode: 'S&P',
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

test('messageActionForRadioMode selects the config for each radio mode family', () => {
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
  const digitalConfig = `
# RUN Messages
F12 Digital Clear,{Action:Clear}
# S&P Messages
F12 Digital Clear,{Action:Clear}
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
  assert.equal(
    messageActionForRadioMode(
      cwConfig,
      voiceConfig,
      'run',
      'F12',
      'DATA',
      digitalConfig,
    ),
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
  assert.equal(cwActionForMessage(config, 'search_and_pounce', 'F12'), null);
  assert.equal(cwActionForMessage(config, 'run', 'F1'), null);
  assert.equal(cwActionForMessage(config, 'run', 'F9'), null);
});

test('correctedEsmCallsignText returns suffix-only or full callsign corrections', () => {
  assert.equal(correctedEsmCallsignText('KB1AWN', 'KB1AWM'), 'AWM');
  assert.equal(correctedEsmCallsignText('KD1AWM', 'KB1AWM'), 'KB1AWM');
  assert.equal(correctedEsmCallsignText('3DA0RU', '3DA0RW'), 'RW');
  assert.equal(correctedEsmCallsignText('K1ABC', 'K1ABC/VE3'), 'K1ABC/VE3');
  assert.equal(correctedEsmCallsignText('K1ABC', 'K1ABC'), '');
});

test('previousContactExchangeAutofill copies non-serial fields from exact callsign match', () => {
  const settings = {
    exchange: [
      {
        id: 'serial',
        label: 'Serial',
        input: { kind: 'serial', max_length: 4 },
        adif: 'STX',
        direction: 'sent',
      },
      {
        id: 'name',
        label: 'Name',
        input: { kind: 'string', max_length: 10 },
        adif: 'NAME',
        direction: 'received',
      },
      {
        id: 'qth',
        label: 'QTH',
        input: { kind: 'string', max_length: 5 },
        adif: 'QTH',
        direction: 'received',
      },
    ],
  };
  const newestContact = {
    adif: {
      CALL: 'K1ABC',
      STX: '123',
      NAME: 'alice',
      QTH: 'ny',
    },
  };

  const result = previousContactExchangeAutofill({
    settings,
    contacts: [
      newestContact,
      { adif: { CALL: 'K1ABC', STX: '122', NAME: 'older', QTH: 'ma' } },
    ],
    callsign: ' k1abc ',
    exchangeValues: { serial: '999', name: '', qth: '' },
    radioMode: 'CW',
  });

  assert.equal(result.matchedContact, newestContact);
  assert.equal(result.changed, true);
  assert.deepEqual(result.copiedFields, ['name', 'qth']);
  assert.deepEqual(result.values, {
    serial: '999',
    name: 'ALICE',
    qth: 'NY',
  });
});

test('previousContactExchangeAutofill preserves user-entered values and requires exact callsign', () => {
  const settings = {
    exchange: [
      {
        id: 'name',
        label: 'Name',
        input: { kind: 'string', max_length: 10 },
        adif: 'NAME',
        direction: 'received',
      },
      {
        id: 'section',
        label: 'Section',
        input: { kind: 'string', max_length: 3 },
        adif: 'ARRL_SECT',
        direction: 'received',
      },
    ],
  };

  const prefixOnly = previousContactExchangeAutofill({
    settings,
    contacts: [{ adif: { CALL: 'K1ABC', NAME: 'Alice', ARRL_SECT: 'SC' } }],
    callsign: 'K1A',
    exchangeValues: { name: '', section: '' },
  });

  assert.equal(prefixOnly.matchedContact, null);
  assert.equal(prefixOnly.changed, false);
  assert.deepEqual(prefixOnly.values, { name: '', section: '' });

  const exact = previousContactExchangeAutofill({
    settings,
    contacts: [{ adif: { CALL: 'k1abc', NAME: 'Alice', ARRL_SECT: 'SC' } }],
    callsign: 'K1ABC',
    exchangeValues: { name: 'BOB', section: '' },
  });

  assert.equal(exact.changed, true);
  assert.deepEqual(exact.copiedFields, ['section']);
  assert.deepEqual(exact.values, { name: 'BOB', section: 'SC' });
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

test('esm callsign corrections preserve the callsign whose exchange was sent', () => {
  assert.deepEqual(
    esmStateAfterCallsignEdit({
      callsign: 'W4MEL',
      runCallsignAttempt: 'W4ME',
      exchangeSentCallsign: 'W4ME',
    }),
    {
      runCallsignAttempt: '',
      exchangeSentCallsign: 'W4ME',
    },
  );
  assert.deepEqual(
    esmStateAfterCallsignEdit({
      callsign: '',
      runCallsignAttempt: 'W4ME',
      exchangeSentCallsign: 'W4ME',
    }),
    {
      runCallsignAttempt: '',
      exchangeSentCallsign: '',
    },
  );

  assert.deepEqual(
    esmEnterAction({
      esmEnabled: true,
      operatingMode: 'Run',
      callsign: 'W4MEL',
      exchangeValid: true,
      exchangeSentCallsign: 'W4ME',
      runCallsignAttempt: '',
    }),
    {
      keys: ['F3'],
      correctionText: 'MEL',
      shouldLog: true,
      nextRunCallsignAttempt: 'W4MEL',
      nextExchangeSentCallsign: 'W4MEL',
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
