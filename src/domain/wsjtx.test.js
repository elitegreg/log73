import assert from 'node:assert/strict';
import test from 'node:test';
import {
  nextAvailableWsjtxPort,
  wsjtxContactFromMessage,
  wsjtxDataModeLocked,
  wsjtxTargetControlVisible,
} from './wsjtx.js';

function adifField(name, value, type = '') {
  return `<${name}:${String(value).length}${type ? `:${type}` : ''}>${value}`;
}

test('nextAvailableWsjtxPort skips ports reserved by enabled radios', () => {
  assert.equal(
    nextAvailableWsjtxPort([
      { wsjtx_enabled: true, wsjtx_port: 2237 },
      { wsjtx_enabled: true, wsjtx_port: 2238 },
      { wsjtx_enabled: false, wsjtx_port: 2239 },
    ]),
    2239,
  );
});

test('nextAvailableWsjtxPort ignores malformed input and honors the minimum', () => {
  assert.equal(nextAvailableWsjtxPort(null, 1), 1024);
});

test('wsjtxDataModeLocked requires an enabled target in DATA mode', () => {
  assert.equal(
    wsjtxDataModeLocked({ wsjtx_enabled: true }, ' data ', true),
    true,
  );
  assert.equal(
    wsjtxDataModeLocked({ wsjtx_enabled: true }, 'DATA', false),
    false,
  );
  assert.equal(wsjtxDataModeLocked({ wsjtx_enabled: true }, 'CW', true), false);
  assert.equal(
    wsjtxDataModeLocked({ wsjtx_enabled: false }, 'DATA', true),
    false,
  );
});

test('WSJT-X target replaces ESM only for enabled DATA mode', () => {
  assert.equal(wsjtxTargetControlVisible(true, 'DATA'), true);
  assert.equal(wsjtxTargetControlVisible(true, 'CW'), false);
  assert.equal(wsjtxTargetControlVisible(false, 'DATA'), false);
});

test('WSJT-X contact preserves every QSO field except combined on-time fields', () => {
  const sourceFields = {
    QSO_DATE: '20240801',
    TIME_ON: '123456',
    QSO_DATE_OFF: '20240801',
    TIME_OFF: '123500',
    STATION_CALLSIGN: 'N0CALL',
    CALL: 'W1AW',
    BAND: '20m',
    FREQ: '14.074',
    MODE: 'MFSK',
    SUBMODE: 'FT8',
    CONTEST_ID: 'WSJTX-CONTEST',
    OPERATOR: 'W1OP',
    COMMENT: '  spaced note  ',
    PROP_MODE: 'TR',
    APP_WSJTX_FOO: 'future-value',
    _PRIVATE: 'private-value',
    ID: 'wsjtx-id',
    APP_LOG73_TX_ID: '1',
  };
  const text = [
    adifField('PROGRAMID', 'WSJT-X'),
    '<EOH>',
    ...Object.entries(sourceFields).map(([name, value]) =>
      adifField(name.toLowerCase(), value, 'S'),
    ),
    '<EOR>',
  ].join('');

  const contact = wsjtxContactFromMessage({
    eventId: 'event-1',
    text,
    logId: 7,
    sessionId: 'session-1',
    operatorCallsign: 'N0OP',
    cabrilloTransmitterId: 0,
  });

  const expectedAdif = { ...sourceFields };
  delete expectedAdif.QSO_DATE;
  delete expectedAdif.TIME_ON;
  expectedAdif.QSO_DATE_TIME_ON = 1722515696;
  assert.deepEqual(contact.adif, expectedAdif);
  assert.deepEqual(contact.meta, {
    status: 'Pending',
    sessionId: 'session-1',
    logId: 7,
    clientId: 'event-1',
    source: 'wsjtx',
    force: true,
  });
});

test('WSJT-X contact adds missing operator and transmitter context only', () => {
  const text =
    '<QSO_DATE:8>20240801<TIME_ON:4>1234<STATION_CALLSIGN:6>N0CALL<CALL:4>W1AW<BAND:3>20m<FREQ:6>14.074<MODE:4>MFSK<EOR>';
  const contact = wsjtxContactFromMessage({
    eventId: 'event-2',
    text,
    logId: 7,
    sessionId: 'session-1',
    operatorCallsign: 'N0OP',
    cabrilloTransmitterId: 0,
  });

  assert.equal(contact.adif.OPERATOR, 'N0OP');
  assert.equal(contact.adif.APP_LOG73_TX_ID, 0);
  assert.equal(contact.adif.FREQ, '14.074');
});

test('WSJT-X contact rejects malformed, incomplete, and multiple records', () => {
  const base = {
    eventId: 'event-3',
    logId: 7,
    sessionId: 'session-1',
  };
  assert.throws(
    () => wsjtxContactFromMessage({ ...base, text: '<CALL:4>W1AW' }),
    /missing EOR/,
  );
  assert.throws(
    () => wsjtxContactFromMessage({ ...base, text: '<CALL:5>W1' }),
    /truncated/,
  );
  const record =
    '<QSO_DATE:8>20240801<TIME_ON:6>123456<STATION_CALLSIGN:6>N0CALL<CALL:4>W1AW<BAND:3>20m<FREQ:6>14.074<MODE:4>MFSK<EOR>';
  assert.throws(
    () => wsjtxContactFromMessage({ ...base, text: record + record }),
    /exactly one QSO record/,
  );
  assert.throws(
    () =>
      wsjtxContactFromMessage({
        ...base,
        text: '<QSO_DATE:8>20240801<TIME_ON:6>123456<EOR>',
      }),
    /STATION_CALLSIGN is required/,
  );
});
