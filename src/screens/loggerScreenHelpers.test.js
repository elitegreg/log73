import assert from 'node:assert/strict';
import test from 'node:test';
import {
  committedBackendContact,
  appendSerialRange,
  mergeContact,
  reserveNextSerial,
  saveLocalContacts,
  saveSerialAllocation,
  serialBatchSize,
  serialRangesRemaining,
  serialRefillRemainingThreshold,
  sortContacts,
  sortContactsByCallsignThenTime,
} from './loggerScreenHelpers.js';

test('sortContacts keeps normal log ordering newest first', () => {
  const contacts = [
    { adif: { CALL: 'K1CCC', QSO_DATE_TIME_ON: 100 } },
    { adif: { CALL: 'K1AAA', QSO_DATE_TIME_ON: 300 } },
    { adif: { CALL: 'K1BBB', QSO_DATE_TIME_ON: 200 } },
  ];

  assert.deepEqual(
    sortContacts(contacts).map((contact) => contact.adif.CALL),
    ['K1AAA', 'K1BBB', 'K1CCC'],
  );
});

test('sortContactsByCallsignThenTime groups callsigns before newest first time', () => {
  const contacts = [
    { adif: { CALL: 'K1BBB', QSO_DATE_TIME_ON: 300 } },
    { adif: { CALL: 'K1AAA', QSO_DATE_TIME_ON: 100 } },
    { adif: { CALL: 'K1BBB', QSO_DATE_TIME_ON: 200 } },
    { adif: { CALL: 'K1AAA', QSO_DATE_TIME_ON: 400 } },
  ];

  assert.deepEqual(
    sortContactsByCallsignThenTime(contacts).map(
      (contact) => `${contact.adif.CALL}:${contact.adif.QSO_DATE_TIME_ON}`,
    ),
    ['K1AAA:400', 'K1AAA:100', 'K1BBB:300', 'K1BBB:200'],
  );
});

test('sortContactsByCallsignThenTime normalizes callsign case', () => {
  const contacts = [
    { adif: { CALL: 'k1bbb', QSO_DATE_TIME_ON: 300 } },
    { adif: { CALL: 'K1AAA', QSO_DATE_TIME_ON: 200 } },
    { adif: { CALL: 'K1BBB', QSO_DATE_TIME_ON: 100 } },
  ];

  assert.deepEqual(
    sortContactsByCallsignThenTime(contacts).map((contact) => contact.adif.CALL),
    ['K1AAA', 'k1bbb', 'K1BBB'],
  );
});

test('sortContactsByCallsignThenTime uses contact id as a final tie breaker', () => {
  const contacts = [
    { meta: { id: 12 }, adif: { CALL: 'K1AAA', QSO_DATE_TIME_ON: 100 } },
    { meta: { id: 10 }, adif: { CALL: 'K1AAA', QSO_DATE_TIME_ON: 100 } },
    {
      meta: { clientId: 'b' },
      adif: { CALL: 'K1AAA', QSO_DATE_TIME_ON: 100 },
    },
  ];

  assert.deepEqual(
    sortContactsByCallsignThenTime(contacts).map(
      (contact) => contact.meta.id ?? contact.meta.clientId,
    ),
    [10, 12, 'b'],
  );
});

test('serial allocation helpers reserve ranges and calculate threshold', () => {
  const allocation = appendSerialRange({ ranges: [] }, 10, 12);
  assert.equal(serialRangesRemaining(allocation), 3);

  const first = reserveNextSerial(allocation);
  assert.equal(first.serial, 10);
  assert.equal(serialRangesRemaining(first.allocation), 2);

  assert.equal(serialBatchSize({ SERIAL_BATCH_SIZE: '25' }), 25);
  assert.equal(serialBatchSize({ SERIAL_BATCH_SIZE: '0' }), 1);
  assert.equal(serialRefillRemainingThreshold(10), 1);
  assert.equal(serialRefillRemainingThreshold(100), 10);
});

test('committedBackendContact assigns meta.clientId from meta.id', () => {
  const committed = committedBackendContact({
    meta: { id: 42 },
    adif: { CALL: 'K1ABC', QSO_DATE_TIME_ON: 100 },
  });

  assert.equal(committed.meta.status, 'Committed');
  assert.equal(committed.meta.clientId, '42');
});

test('mergeContact updates pending contact and rekeys committed meta.clientId to meta.id', () => {
  const pending = {
    meta: {
      clientId: 'local-123',
      status: 'Pending',
    },
    adif: {
      CALL: 'K1ABC',
      QSO_DATE_TIME_ON: 100,
    },
  };

  const merged = mergeContact([pending], {
    meta: {
      id: 77,
      clientId: 'local-123',
      status: 'Committed',
    },
    adif: {
      CALL: 'K1ABC',
      QSO_DATE_TIME_ON: 100,
    },
  });

  assert.equal(merged.length, 1);
  assert.equal(merged[0].meta.id, 77);
  assert.equal(merged[0].meta.status, 'Committed');
  assert.equal(merged[0].meta.clientId, '77');
});

test('saveLocalContacts catches storage write failures and reports once', async () => {
  const fetchCalls = [];
  globalThis.window = {
    location: { href: 'https://example.test/logger' },
    navigator: { userAgent: 'node-test' },
  };
  globalThis.fetch = async (url, options = {}) => {
    fetchCalls.push({ url, options });
    return {
      ok: true,
      json: async () => ({ ok: true }),
    };
  };
  globalThis.localStorage = {
    setItem() {
      throw new Error('quota exceeded');
    },
  };

  const resultA = saveLocalContacts(7, [
    {
      meta: { status: 'Pending' },
      adif: { CALL: 'K1ABC', QSO_DATE_TIME_ON: 100 },
    },
  ]);
  const resultB = saveLocalContacts(7, []);
  await Promise.resolve();

  assert.equal(resultA, false);
  assert.equal(resultB, false);
  assert.equal(fetchCalls.length, 1);
  assert.equal(fetchCalls[0].url, '/api/client-errors');
});

test('saveSerialAllocation catches storage write failures and reports once', async () => {
  const fetchCalls = [];
  globalThis.window = {
    location: { href: 'https://example.test/logger' },
    navigator: { userAgent: 'node-test' },
  };
  globalThis.fetch = async (url, options = {}) => {
    fetchCalls.push({ url, options });
    return {
      ok: true,
      json: async () => ({ ok: true }),
    };
  };
  globalThis.localStorage = {
    setItem() {
      throw new Error('private mode denied');
    },
  };

  const resultA = saveSerialAllocation(7, 'STX', 'instance-1', {
    ranges: [{ next: 10, end: 12 }],
  });
  const resultB = saveSerialAllocation(7, 'STX', 'instance-1', { ranges: [] });
  await Promise.resolve();

  assert.equal(resultA, false);
  assert.equal(resultB, false);
  assert.equal(fetchCalls.length, 1);
  assert.equal(fetchCalls[0].url, '/api/client-errors');
});
