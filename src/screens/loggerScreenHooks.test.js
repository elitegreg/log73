import assert from 'node:assert/strict';
import test from 'node:test';
import {
  formatSocketDebugDetails,
  readSocketDebugPanelEnabled,
  websocketReadyStateLabel,
} from './loggerScreen/backendSocketController.js';
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
