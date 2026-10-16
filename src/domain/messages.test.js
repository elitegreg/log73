import {
  loadMessageEntriesFixture,
  loadMessageModesFixture,
} from '../../tests/fixtures/domain/loadFixture.js';
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

const fixture = loadMessageEntriesFixture();
const modeFixture = loadMessageModesFixture();

test('shared message entry contract has the supported schema and cases', () => {
  assert.equal(fixture.version, 1);
  assert.ok(fixture.parser_cases.length > 0);
  assert.ok(fixture.action_cases.length > 0);
  assert.ok(fixture.default_configs.length > 0);
  assert.equal(modeFixture.version, 1);
  assert.ok(modeFixture.header_cases.length > 0);
});

for (const c of fixture.parser_cases) {
  test(`shared message entries: ${c.name}`, () => {
    assert.deepEqual(parseMessageEntries(c.config), c.entries);
  });
}

for (const c of fixture.action_cases) {
  test(`shared message action: ${c.name}`, () => {
    assert.equal(actionFromTemplate(c.template), c.action);
    if (c.action !== null) {
      // Configure multiline values through the existing whitespace-permitted
      // action syntax; config entries themselves occupy one line.
      const target = c.template.trim().replaceAll('\n', ' ');
      const config = `# RUN Messages\nF12 Clear,${target}`;
      assert.equal(
        messageActionForConfig(config, 'run', 'F12'),
        c.action.replaceAll('\n', ' '),
      );
      assert.equal(renderMessageForConfig(config, 'run', 'F12'), null);
    }
  });
}

for (const c of fixture.default_configs) {
  test(`shared default message entries: ${c.kind}`, () => {
    assert.deepEqual(parseMessageEntries(c.config), c.entries);
  });
}

for (const c of modeFixture.header_cases) {
  test(`shared message header integration: ${c.name}`, () => {
    const config = `${c.input ?? ''}\nF1 Label,target`;
    assert.deepEqual(
      parseMessageEntries(config),
      c.mode === null
        ? []
        : [{ mode: c.mode, key: 'F1', label: 'Label', target: 'target' }],
    );
  });
}

test('frontend template substitution preserves literal syntax and missing-field behavior', () => {
  assert.equal(
    renderMessageTemplate(
      ' {CALL}|{STX}|{ENABLED}|{EMPTY}|{NULL}|{MISSING}|{call}|{1BAD}|{BAD-NAME}|{UNCLOSED ',
      { CALL: ' K1ABC ', STX: 0, ENABLED: false, EMPTY: '', NULL: null },
    ),
    'K1ABC|0|false||||{call}|{1BAD}|{BAD-NAME}|{UNCLOSED',
  );
  assert.equal(renderMessageTemplate(null), '');
  assert.equal(
    renderMessageTemplate('{RST_SENT}/{SENTRSTCUT}', { RST_SENT: ' 5n9 ' }),
    '5NN/5NN',
  );
  assert.equal(renderMessageTemplate('{RST_SENT}/{SENTRSTCUT}'), '/');
});

test('malformed action tokens remain ordinary configured text', () => {
  const config =
    '# RUN Messages\nF1 Empty,{Action:}\nF2 Blank,{Action: }\nF3 Suffix,{Action:Clear} TU';
  assert.equal(renderMessageForConfig(config, 'run', 'F1'), '{Action:}');
  assert.equal(renderMessageForConfig(config, 'run', 'F2'), '{Action: }');
  assert.equal(
    renderMessageForConfig(config, 'run', 'F3'),
    '{Action:Clear} TU',
  );
});

test('duplicate entries retain order and frontend lookup selects the first', () => {
  const config = '# RUN Messages\nF1 First,ONE\nF1 Second,TWO';
  assert.equal(messageEntryForConfig(config, 'run', 'F1').label, 'First');
  assert.equal(renderMessageForConfig(config, 'run', 'F1'), 'ONE');
});
