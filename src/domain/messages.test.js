import assert from 'node:assert/strict';
import test from 'node:test';
import {
  actionFromTemplate,
  completionTrackedTextRequest,
  messageActionForConfig,
  messageEntryForConfig,
  messageSendBatches,
  parseMessageEntries,
  renderMessageForConfig,
  renderMessageTemplate,
} from './messages.js';

const TEST_CONFIG = `
# RUN Messages
F1 Cq,CQ TEST
F12 Clear,{Action:Clear}
# S&P Messages
F12 Clear,CQ
`;

test('message helpers parse entries and action tokens', () => {
  const entries = parseMessageEntries(TEST_CONFIG);
  assert.equal(entries.length, 3);
  assert.equal(entries[0].mode, 'run');
  assert.equal(entries[0].key, 'F1');
  assert.equal(entries[0].label, 'Cq');
  assert.equal(actionFromTemplate('{Action:Clear}'), 'Clear');
  assert.equal(messageActionForConfig(TEST_CONFIG, 'run', 'F12'), 'Clear');
  assert.equal(messageActionForConfig(TEST_CONFIG, 's&p', 'F12'), null);
  assert.equal(
    messageEntryForConfig(TEST_CONFIG, 'RUN', 'f1')?.target,
    'CQ TEST',
  );
});

test('message templates render logger state keys and cut-number fields', () => {
  const fields = {
    STATION_CALLSIGN: 'K1ABC',
    CALL: 'n0call',
    RST_SENT: '599',
    STX: 73,
    NAME: ' Greg ',
    EXCH: '5NN 73 GREG',
  };

  assert.equal(
    renderMessageTemplate(
      '  {CALL} {RST_SENT} {SENTRSTCUT} {STX} {NAME} {EXCH} {MISSING}  ',
      fields,
    ),
    'n0call 5NN 5NN 73 Greg 5NN 73 GREG',
  );
  assert.equal(
    renderMessageTemplate('{Action:Clear}', fields),
    '{Action:Clear}',
  );
});

test('configured messages render by mode and do not render actions', () => {
  const config = `
# RUN Messages
F1 Cq,CQ {STATION_CALLSIGN} {EXCH}
F12 Clear,{Action:Clear}
# S&P Messages
F1 Qrl?,{OPERATOR}/QRL.wav
`;

  assert.equal(
    renderMessageForConfig(config, 'run', 'F1', {
      STATION_CALLSIGN: 'K1ABC',
      EXCH: '5NN EMA',
    }),
    'CQ K1ABC 5NN EMA',
  );
  assert.equal(
    renderMessageForConfig(config, 's&p', 'F1', { OPERATOR: 'N0CALL' }),
    'N0CALL/QRL.wav',
  );
  assert.equal(renderMessageForConfig(config, 'run', 'F12', {}), null);
  assert.equal(renderMessageForConfig(config, 'run', 'F9', {}), null);
});

test('digital message definitions replace NEWLINE with a literal newline', () => {
  const config = `
# RUN Messages
F1 Multi,Line one{NEWLINE}Line two {CALL}
# S&P Messages
F1 Multi,First{NEWLINE}Second
`;

  assert.equal(
    renderMessageForConfig(
      config,
      'run',
      'F1',
      { CALL: 'K1ABC' },
      {
        replaceNewline: true,
      },
    ),
    'Line one\nLine two K1ABC',
  );
  assert.equal(
    renderMessageForConfig(config, 'run', 'F1', {}, { replaceNewline: false }),
    'Line oneLine two',
  );
});

test('message batches join text messages and separate voice files', () => {
  const messages = [
    { key: 'F5', text: 'K1ABC' },
    { key: 'F2', text: '5NN EMA' },
  ];

  assert.deepEqual(messageSendBatches(messages), [
    { keys: ['F5', 'F2'], text: 'K1ABC 5NN EMA' },
  ]);
  assert.deepEqual(messageSendBatches(messages, true), [
    { keys: ['F5'], text: 'K1ABC' },
    { keys: ['F2'], text: '5NN EMA' },
  ]);
  assert.deepEqual(completionTrackedTextRequest('request-1', 'CQ TEST'), {
    request_id: 'request-1',
    text: 'CQ TEST',
    wait_for_completion: true,
  });
});
