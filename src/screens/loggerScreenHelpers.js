import { reportClientErrorLater } from '../lib/errorReporting.js';

export const CONTACTS_STORAGE_KEY = 'log73.contacts';
export const SESSION_STORAGE_KEY = 'log73.session_id';
export const SERIAL_ALLOCATION_STORAGE_PREFIX = 'log73.serial.v1';
export const SERIAL_INSTANCE_STORAGE_KEY = 'log73.serial.instance_id';
export const SERIAL_BATCH_SIZE_PARAM = 'SERIAL_BATCH_SIZE';
export const BACKEND_WS_INITIAL_RECONNECT_DELAY_MS = 2000;
export const BACKEND_WS_MAX_RECONNECT_DELAY_MS = 16000;
export const BACKEND_WS_IDLE_PING_DELAY_MS = 15000;
export const BACKEND_WS_PING_TIMEOUT_MS = 5000;
export const CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS = 2000;
export const CONTACTS_LOAD_MAX_RETRY_DELAY_MS = 16000;
export const CONTACTS_PAGE_SIZE = 200;
export const CONTACT_COMMIT_RETRY_DELAY_MS = 5000;
export const SERIAL_ALLOCATION_RETRY_DELAY_MS = 5000;
export const DEFAULT_SERIAL_BATCH_SIZE = 10;
export const MAX_SERIAL_BATCH_SIZE = 1000;
export const DEFAULT_RADIO_STATE = { mode: 'CW', frequency_hz: 14000000 };
export const EMPTY_SCORE_SUMMARY = {
  qsoCount: 0,
  multipliers: 0,
  bonusPoints: 0,
  score: 0,
};

export function contactSortValue(contact) {
  if (typeof contact.QSO_DATE_TIME_ON === 'number')
    return contact.QSO_DATE_TIME_ON;
  if (typeof contact._time_on_epoch === 'number') return contact._time_on_epoch;
  const date = String(contact.QSO_DATE ?? '');
  const time = String(contact.TIME_ON ?? '');
  const parsed = Date.UTC(
    Number.parseInt(date.slice(0, 4), 10),
    Number.parseInt(date.slice(4, 6), 10) - 1,
    Number.parseInt(date.slice(6, 8), 10),
    Number.parseInt(time.slice(0, 2), 10),
    Number.parseInt(time.slice(2, 4), 10),
    Number.parseInt(time.slice(4, 6), 10),
  );
  return Number.isFinite(parsed) ? Math.floor(parsed / 1000) : 0;
}

export function sortContacts(contacts) {
  return [...contacts].sort(
    (a, b) => contactSortValue(b) - contactSortValue(a),
  );
}

function contactCallsign(contact) {
  return String(contact?.CALL ?? contact?.Call ?? '')
    .trim()
    .toUpperCase();
}

function compareText(left, right) {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function compareContactIds(left, right) {
  if (left?._id !== undefined && right?._id !== undefined) {
    return Number(left._id) - Number(right._id);
  }
  if (left?._id !== undefined) return -1;
  if (right?._id !== undefined) return 1;
  return compareText(left?._client_id ?? '', right?._client_id ?? '');
}

export function sortContactsByCallsignThenTime(contacts) {
  return [...contacts].sort((a, b) => {
    const callsignComparison = compareText(
      contactCallsign(a),
      contactCallsign(b),
    );
    if (callsignComparison !== 0) return callsignComparison;

    const timeComparison = contactSortValue(b) - contactSortValue(a);
    if (timeComparison !== 0) return timeComparison;

    return compareContactIds(a, b);
  });
}

export function normalizeContact(contact) {
  const nextContact = { ...contact };
  if (
    nextContact._status === 'Committed' &&
    nextContact._id !== undefined &&
    nextContact._id !== null
  ) {
    nextContact._client_id = String(nextContact._id);
  }
  if (typeof nextContact.QSO_DATE_TIME_ON !== 'number') {
    const epoch = contactSortValue(nextContact);
    if (epoch > 0) nextContact.QSO_DATE_TIME_ON = epoch;
  }
  if (nextContact.FREQ !== undefined) {
    const frequency = Number.parseFloat(String(nextContact.FREQ));
    if (Number.isFinite(frequency))
      nextContact.FREQ = Math.round(
        Math.abs(frequency) < 1000000 ? frequency * 1000000 : frequency,
      );
  }
  delete nextContact.QSO_DATE;
  delete nextContact.TIME_ON;
  delete nextContact._time_on_epoch;
  return nextContact;
}

export function shouldPersistLocally(contact) {
  return (
    contact._status === 'Pending' ||
    contact._status === 'Updating' ||
    contact._status === 'Failed'
  );
}
export function contactStorageKey(logId) {
  return `${CONTACTS_STORAGE_KEY}.${logId}`;
}

export function loadLocalContacts(logId) {
  try {
    const parsed = JSON.parse(
      localStorage.getItem(contactStorageKey(logId)) ?? '[]',
    );
    return Array.isArray(parsed)
      ? sortContacts(parsed.map(normalizeContact).filter(shouldPersistLocally))
      : [];
  } catch (error) {
    reportClientErrorLater({
      source: 'loggerScreenHelpers.loadLocalContacts',
      message: 'Unable to load locally stored contacts.',
      error,
      details: { logId },
    });
    return [];
  }
}

export function saveLocalContacts(logId, contacts) {
  localStorage.setItem(
    contactStorageKey(logId),
    JSON.stringify(contacts.filter(shouldPersistLocally)),
  );
}

export function serialBatchSize(contestParams = {}) {
  const parsed = Number.parseInt(
    String(
      contestParams?.[SERIAL_BATCH_SIZE_PARAM] ?? DEFAULT_SERIAL_BATCH_SIZE,
    ),
    10,
  );
  if (!Number.isFinite(parsed)) return DEFAULT_SERIAL_BATCH_SIZE;
  return Math.min(Math.max(parsed, 1), MAX_SERIAL_BATCH_SIZE);
}

export function serialRefillRemainingThreshold(batchSize) {
  return Math.max(
    1,
    Math.floor(serialBatchSize({ [SERIAL_BATCH_SIZE_PARAM]: batchSize }) * 0.1),
  );
}

export function serialFieldTypeKind(field) {
  return String(field?.type ?? 'String')
    .split(':')[0]
    .trim()
    .toUpperCase();
}

export function sentSerialField(settings) {
  return (settings?.exchange ?? []).find(
    (field) =>
      field?.is_sent === true && serialFieldTypeKind(field) === 'SERIAL',
  );
}

export function getSerialInstanceId() {
  let instanceId = sessionStorage.getItem(SERIAL_INSTANCE_STORAGE_KEY);
  if (instanceId) return instanceId;
  instanceId = window.crypto?.randomUUID
    ? window.crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  sessionStorage.setItem(SERIAL_INSTANCE_STORAGE_KEY, instanceId);
  return instanceId;
}

export function serialAllocationStorageKey(logId, fieldAdif, instanceId) {
  return `${SERIAL_ALLOCATION_STORAGE_PREFIX}.${logId}.${fieldAdif}.${instanceId}`;
}

export function loadSerialAllocation(logId, fieldAdif, instanceId) {
  try {
    const parsed = JSON.parse(
      localStorage.getItem(
        serialAllocationStorageKey(logId, fieldAdif, instanceId),
      ) ?? '{}',
    );
    return {
      ranges: normalizeSerialRanges(parsed?.ranges),
    };
  } catch (error) {
    reportClientErrorLater({
      source: 'loggerScreenHelpers.loadSerialAllocation',
      message: 'Unable to load locally stored serial allocation.',
      error,
      details: { logId, fieldAdif },
    });
    return { ranges: [] };
  }
}

export function saveSerialAllocation(logId, fieldAdif, instanceId, allocation) {
  localStorage.setItem(
    serialAllocationStorageKey(logId, fieldAdif, instanceId),
    JSON.stringify({ ranges: normalizeSerialRanges(allocation?.ranges) }),
  );
}

export function normalizeSerialRanges(ranges) {
  return (Array.isArray(ranges) ? ranges : [])
    .map((range) => ({
      next: Number.parseInt(String(range?.next), 10),
      end: Number.parseInt(String(range?.end), 10),
    }))
    .filter(
      (range) =>
        Number.isFinite(range.next) &&
        Number.isFinite(range.end) &&
        range.next > 0 &&
        range.end >= range.next,
    )
    .sort((left, right) => left.next - right.next);
}

export function serialRangesRemaining(allocation) {
  return normalizeSerialRanges(allocation?.ranges).reduce(
    (total, range) => total + (range.end - range.next + 1),
    0,
  );
}

export function appendSerialRange(allocation, start, end) {
  const parsedStart = Number.parseInt(String(start), 10);
  const parsedEnd = Number.parseInt(String(end), 10);
  return {
    ranges: normalizeSerialRanges([
      ...(allocation?.ranges ?? []),
      { next: parsedStart, end: parsedEnd },
    ]),
  };
}

export function reserveNextSerial(allocation) {
  const ranges = normalizeSerialRanges(allocation?.ranges);
  const [firstRange, ...remainingRanges] = ranges;
  if (!firstRange) return { serial: null, allocation: { ranges } };

  const serial = firstRange.next;
  const nextRange = { ...firstRange, next: firstRange.next + 1 };
  const nextRanges =
    nextRange.next <= nextRange.end
      ? [nextRange, ...remainingRanges]
      : remainingRanges;
  return { serial, allocation: { ranges: nextRanges } };
}

export function committedBackendContact(contact) {
  return normalizeContact({
    ...contact,
    _status: contact._status ?? 'Committed',
  });
}
export function createSessionId() {
  return window.crypto?.randomUUID
    ? window.crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}
export function getSessionId() {
  const existingSessionId = localStorage.getItem(SESSION_STORAGE_KEY);
  if (existingSessionId) return existingSessionId;
  const sessionId = createSessionId();
  localStorage.setItem(SESSION_STORAGE_KEY, sessionId);
  return sessionId;
}

export function contactMatches(left, right) {
  if (left._id !== undefined && right._id !== undefined)
    return String(left._id) === String(right._id);
  if (left._client_id && right._client_id)
    return left._client_id === right._client_id;
  return false;
}

export function contactIdentifier(contact) {
  if (contact._id !== undefined) return `id:${contact._id}`;
  if (contact._client_id) return `client:${contact._client_id}`;
  return null;
}

export function mergeContact(contacts, contact) {
  const committedContact = committedBackendContact(contact);
  const index = contacts.findIndex((currentContact) =>
    contactMatches(currentContact, contact),
  );
  if (index === -1) return sortContacts([...contacts, committedContact]);
  const nextContacts = [...contacts];
  nextContacts[index] = {
    ...nextContacts[index],
    ...committedContact,
    _error: undefined,
  };
  if (nextContacts[index]._status === 'Committed') {
    delete nextContacts[index]._force;
  }
  return sortContacts(nextContacts);
}

export function markContactFailed(contacts, failedContact, error) {
  return sortContacts(
    contacts.map((contact) =>
      contactMatches(contact, failedContact)
        ? { ...contact, _status: 'Failed', _error: error }
        : contact,
    ),
  );
}
