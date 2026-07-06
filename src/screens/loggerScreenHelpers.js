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
export const DEFAULT_RADIO_STATE = {
  mode: 'CW',
  frequency_hz: 14000000,
  rit_offset_hz: 0,
};
export const EMPTY_SCORE_SUMMARY = {
  qsoCount: 0,
  multipliers: 0,
  bonusPoints: 0,
  score: 0,
};

const reportedStorageWriteFailures = new Set();

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function contactMeta(contact) {
  return isObject(contact?.meta) ? contact.meta : {};
}

export function contactAdif(contact) {
  return isObject(contact?.adif) ? contact.adif : {};
}

export function metaValue(contact, key) {
  const meta = contactMeta(contact);
  return Object.prototype.hasOwnProperty.call(meta, key) ? meta[key] : undefined;
}

export function adifValue(contact, key) {
  const adif = contactAdif(contact);
  return Object.prototype.hasOwnProperty.call(adif, key) ? adif[key] : undefined;
}

export function contactSortValue(contact) {
  const qsoDateTimeOn = adifValue(contact, 'QSO_DATE_TIME_ON');
  return typeof qsoDateTimeOn === 'number' ? qsoDateTimeOn : 0;
}

export function sortContacts(contacts) {
  return [...contacts].sort(
    (a, b) => contactSortValue(b) - contactSortValue(a),
  );
}

function contactCallsign(contact) {
  return String(adifValue(contact, 'CALL') ?? '').trim().toUpperCase();
}

function compareText(left, right) {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function compareContactIds(left, right) {
  const leftId = metaValue(left, 'id');
  const rightId = metaValue(right, 'id');
  if (leftId !== undefined && rightId !== undefined) {
    return Number(leftId) - Number(rightId);
  }
  if (leftId !== undefined) return -1;
  if (rightId !== undefined) return 1;
  return compareText(
    String(metaValue(left, 'clientId') ?? ''),
    String(metaValue(right, 'clientId') ?? ''),
  );
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
  const meta = isObject(contact?.meta) ? { ...contact.meta } : {};
  const adif = isObject(contact?.adif) ? { ...contact.adif } : {};

  if (
    meta.status === 'Committed' &&
    meta.id !== undefined &&
    meta.id !== null
  ) {
    meta.clientId = String(meta.id);
  }
  if (adif.FREQ !== undefined) {
    const frequency = Number.parseFloat(String(adif.FREQ));
    if (Number.isFinite(frequency)) {
      adif.FREQ = Math.round(
        Math.abs(frequency) < 1000000 ? frequency * 1000000 : frequency,
      );
    }
  }

  return { meta, adif };
}

export function shouldPersistLocally(contact) {
  const status = metaValue(contact, 'status');
  return status === 'Pending' || status === 'Updating' || status === 'Failed';
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

function reportStorageWriteFailureOnce(kind, message, error, details) {
  if (reportedStorageWriteFailures.has(kind)) return;
  reportedStorageWriteFailures.add(kind);
  reportClientErrorLater({
    source: `loggerScreenHelpers.${kind}`,
    message,
    error,
    details,
  });
}

export function saveLocalContacts(logId, contacts) {
  try {
    localStorage.setItem(
      contactStorageKey(logId),
      JSON.stringify(contacts.filter(shouldPersistLocally)),
    );
    return true;
  } catch (error) {
    reportStorageWriteFailureOnce(
      'saveLocalContacts',
      'Unable to save locally stored contacts. Offline caching is degraded.',
      error,
      { logId },
    );
    return false;
  }
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
  try {
    localStorage.setItem(
      serialAllocationStorageKey(logId, fieldAdif, instanceId),
      JSON.stringify({ ranges: normalizeSerialRanges(allocation?.ranges) }),
    );
    return true;
  } catch (error) {
    reportStorageWriteFailureOnce(
      'saveSerialAllocation',
      'Unable to save locally stored serial allocation. Offline caching is degraded.',
      error,
      { logId, fieldAdif },
    );
    return false;
  }
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
  const normalized = normalizeContact(contact);
  if (!normalized.meta.status) normalized.meta.status = 'Committed';
  if (
    normalized.meta.status === 'Committed' &&
    normalized.meta.id !== undefined &&
    normalized.meta.id !== null
  ) {
    normalized.meta.clientId = String(normalized.meta.id);
  }
  return normalized;
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
  const leftId = metaValue(left, 'id');
  const rightId = metaValue(right, 'id');
  if (leftId !== undefined && rightId !== undefined)
    return String(leftId) === String(rightId);
  const leftClientId = metaValue(left, 'clientId');
  const rightClientId = metaValue(right, 'clientId');
  if (leftClientId && rightClientId) return leftClientId === rightClientId;
  return false;
}

export function contactIdentifier(contact) {
  const id = metaValue(contact, 'id');
  if (id !== undefined) return `id:${id}`;
  const clientId = metaValue(contact, 'clientId');
  if (clientId) return `client:${clientId}`;
  return null;
}

export function mergeContact(contacts, contact) {
  const committedContact = committedBackendContact(contact);
  const index = contacts.findIndex((currentContact) =>
    contactMatches(currentContact, contact),
  );
  if (index === -1) return sortContacts([...contacts, committedContact]);
  const nextContacts = [...contacts];
  const mergedMeta = {
    ...contactMeta(nextContacts[index]),
    ...contactMeta(committedContact),
    error: undefined,
  };
  const mergedAdif = {
    ...contactAdif(nextContacts[index]),
    ...contactAdif(committedContact),
  };
  if (mergedMeta.status === 'Committed') {
    delete mergedMeta.force;
  }
  nextContacts[index] = { meta: mergedMeta, adif: mergedAdif };
  return sortContacts(nextContacts);
}

export function markContactFailed(contacts, failedContact, error) {
  return sortContacts(
    contacts.map((contact) =>
      contactMatches(contact, failedContact)
        ? {
            meta: {
              ...contactMeta(contact),
              status: 'Failed',
              error,
            },
            adif: { ...contactAdif(contact) },
          }
        : contact,
    ),
  );
}
