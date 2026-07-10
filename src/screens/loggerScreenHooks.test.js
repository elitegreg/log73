import assert from 'node:assert/strict';
import test from 'node:test';
import {
  formatSocketDebugDetails,
  logSocketDebug,
  readSocketDebugPanelEnabled,
  websocketReadyStateLabel,
} from './loggerScreen/backendSocketController.js';
import { createBandMapSpotStore } from '../domain/bandMap.js';
import {
  applyBandMapSequenceMessage,
  isBandMapSequenceMessage,
  visibleBandMapSpotStoreForCurrentBand,
} from './loggerScreen/useBandMap.js';
import {
  mergeCommittedPage,
  mergeResetCommittedPage,
  nextContactToCommit,
} from './loggerScreen/contactsOutboxState.js';
import {
  shouldRequestSerialRefill,
  unavailableSerialMessage,
} from './loggerScreen/serialAllocatorState.js';

test('websocket debug helpers format labels and clamp detail length', () => {
  assert.equal(websocketReadyStateLabel(0), 'connecting');
  assert.equal(websocketReadyStateLabel(99), 'unknown(99)');

  const longDetails = formatSocketDebugDetails({ text: 'x'.repeat(400) });
  assert.match(longDetails, /\.\.\.$/);
  assert.equal(formatSocketDebugDetails({}), '');
});

test('readSocketDebugPanelEnabled honors query string and persists preference', () => {
  const storage = new Map();
  const win = {
    location: { search: '?socket_debug=1' },
    localStorage: {
      setItem(key, value) {
        storage.set(key, value);
      },
      getItem(key) {
        return storage.get(key) ?? null;
      },
      removeItem(key) {
        storage.delete(key);
      },
    },
  };

  assert.equal(readSocketDebugPanelEnabled(win), true);
  assert.equal(storage.get('log73.socket_debug_panel'), '1');

  win.location.search = '?socket_debug=0';
  assert.equal(readSocketDebugPanelEnabled(win), false);
  assert.equal(storage.has('log73.socket_debug_panel'), false);

  storage.set('log73.socket_debug_panel', '1');
  win.location.search = '';
  assert.equal(readSocketDebugPanelEnabled(win), true);
});

test('logSocketDebug only writes to the console when socket debug is enabled', () => {
  const calls = [];
  const logger = {
    debug(...args) {
      calls.push(args);
    },
  };

  logSocketDebug(false, { event: 'socket_open' }, logger);
  logSocketDebug(true, { event: 'socket_open' }, logger);

  assert.deepEqual(calls, [
    ['[LoggerScreen websocket]', { event: 'socket_open' }],
  ]);
});

test('contacts outbox state merges committed pages without dropping local pending contacts', () => {
  const currentContacts = [
    {
      meta: { id: 1, status: 'Committed' },
      adif: { CALL: 'K1AAA', QSO_DATE_TIME_ON: 100 },
    },
    {
      meta: { clientId: 'local-1', status: 'Pending' },
      adif: { CALL: 'K1PENDING', QSO_DATE_TIME_ON: 150 },
    },
  ];
  const merged = mergeCommittedPage(currentContacts, [
    {
      meta: { id: 1, status: 'Committed' },
      adif: { CALL: 'K1AAA', COMMENT: 'updated', QSO_DATE_TIME_ON: 100 },
    },
    {
      meta: { id: 2, status: 'Committed' },
      adif: { CALL: 'K1BBB', QSO_DATE_TIME_ON: 200 },
    },
  ]);

  assert.equal(merged.length, 3);
  assert.equal(
    merged.find((contact) => contact.meta.id === 1).adif.COMMENT,
    'updated',
  );
  assert.equal(
    merged.find((contact) => contact.meta.clientId === 'local-1').adif.CALL,
    'K1PENDING',
  );
});

test('contacts outbox state resets committed contacts but preserves local drafts', () => {
  const merged = mergeResetCommittedPage(
    [
      {
        meta: { id: 1, status: 'Committed' },
        adif: { CALL: 'OLD', QSO_DATE_TIME_ON: 10 },
      },
      {
        meta: { clientId: 'draft', status: 'Pending' },
        adif: { CALL: 'DRAFT', QSO_DATE_TIME_ON: 20 },
      },
    ],
    [{ meta: { id: 2 }, adif: { CALL: 'NEW', QSO_DATE_TIME_ON: 30 } }],
  );

  assert.deepEqual(
    merged.map((contact) => contact.meta.id ?? contact.meta.clientId),
    [2, 'draft'],
  );
});

test('contacts outbox state picks next non-committing pending or updating contact', () => {
  const contacts = [
    {
      meta: { clientId: 'skip', status: 'Pending' },
      adif: { CALL: 'K1AAA' },
    },
    {
      meta: { clientId: 'take', status: 'Pending' },
      adif: { CALL: 'K1BBB' },
    },
  ];

  const next = nextContactToCommit(contacts, new Set(['skip']));
  assert.equal(next.meta.clientId, 'take');
});

test('band map sequence helpers apply live updates and detect gaps', () => {
  const initialStore = createBandMapSpotStore();
  const upsert = applyBandMapSequenceMessage({
    store: initialStore,
    sequence: 0,
    message: {
      type: 'bandmap_spot',
      sequence: 1,
      spot: {
        id: 9,
        frequency_hz: 14074000,
        call_dx: 'K1ABC',
        call_de: 'N0CALL',
      },
    },
  });

  assert.equal(upsert.needsRefresh, false);
  assert.equal(upsert.sequence, 1);
  assert.equal(upsert.store.sortedSpots.length, 1);

  const stale = applyBandMapSequenceMessage({
    store: upsert.store,
    sequence: upsert.sequence,
    message: { type: 'bandmap_sequence', sequence: 1 },
  });
  assert.equal(stale.needsRefresh, false);
  assert.equal(stale.sequence, 1);

  const gap = applyBandMapSequenceMessage({
    store: upsert.store,
    sequence: upsert.sequence,
    message: { type: 'bandmap_sequence', sequence: 3 },
  });
  assert.equal(gap.needsRefresh, true);
  assert.equal(gap.messageSequence, 3);
});

test('band map sequence helper removes spots and accepts sequence-only updates', () => {
  const withSpot = createBandMapSpotStore([
    {
      id: 9,
      frequency_hz: 14074000,
      call_dx: 'K1ABC',
      call_de: 'N0CALL',
    },
  ]);
  const sequenceOnly = applyBandMapSequenceMessage({
    store: withSpot,
    sequence: 4,
    message: { type: 'bandmap_sequence', sequence: 5 },
  });
  assert.equal(sequenceOnly.needsRefresh, false);
  assert.equal(sequenceOnly.store.sortedSpots.length, 1);
  assert.equal(sequenceOnly.sequence, 5);

  const deleted = applyBandMapSequenceMessage({
    store: withSpot,
    sequence: 5,
    message: { type: 'bandmap_spot_deleted', sequence: 6, id: 9 },
  });
  assert.equal(deleted.needsRefresh, false);
  assert.equal(deleted.sequence, 6);
  assert.equal(deleted.store.sortedSpots.length, 0);

  assert.equal(isBandMapSequenceMessage({ type: 'bandmap_spot' }), true);
  assert.equal(isBandMapSequenceMessage({ type: 'bandmap_sequence' }), true);
  assert.equal(isBandMapSequenceMessage({ type: 'pong' }), false);
});

test('band map visible store only shows spots on the current band', () => {
  const store = createBandMapSpotStore([
    { id: 1, frequency_hz: 14074000, call_dx: 'K1ABC', call_de: 'N0CALL' },
    { id: 2, frequency_hz: 21074000, call_dx: 'K2ABC', call_de: 'N0CALL' },
  ]);
  const settings = {
    band_catalog: [
      { name: '20m', lowerHz: 14000000, upperHz: 14350000 },
      { name: '15m', lowerHz: 21000000, upperHz: 21450000 },
    ],
  };

  assert.equal(store.sortedSpots.length, 2);

  const visible20m = visibleBandMapSpotStoreForCurrentBand({
    store,
    settings,
    radioFrequencyHz: 14025000,
  });
  assert.deepEqual(
    visible20m.sortedSpots.map((spot) => spot.id),
    ['1'],
  );

  const visible15m = visibleBandMapSpotStoreForCurrentBand({
    store,
    settings,
    radioFrequencyHz: 21025000,
  });
  assert.deepEqual(
    visible15m.sortedSpots.map((spot) => spot.id),
    ['2'],
  );
});

test('band map visible store returns no spots when current band is unknown', () => {
  const store = createBandMapSpotStore([
    { id: 1, frequency_hz: 14074000, call_dx: 'K1ABC', call_de: 'N0CALL' },
  ]);

  const visible = visibleBandMapSpotStoreForCurrentBand({
    store,
    settings: {
      band_catalog: [{ name: '20m', lowerHz: 14000000, upperHz: 14350000 }],
    },
    radioFrequencyHz: 5000000,
  });

  assert.equal(store.sortedSpots.length, 1);
  assert.equal(visible.sortedSpots.length, 0);
});

test('serial allocator state detects refill boundaries and user-facing messages', () => {
  assert.equal(
    shouldRequestSerialRefill({ current: null, remaining: 5, threshold: 1 }),
    true,
  );
  assert.equal(
    shouldRequestSerialRefill({ current: 10, remaining: 1, threshold: 1 }),
    true,
  );
  assert.equal(
    shouldRequestSerialRefill({ current: 10, remaining: 2, threshold: 1 }),
    false,
  );

  assert.equal(
    unavailableSerialMessage(null, 0),
    'No serial numbers are currently available. Retrying backend allocation.',
  );
  assert.equal(
    unavailableSerialMessage(44, 3),
    'Serial number refill failed; 3 reserved serial numbers remain.',
  );
});
