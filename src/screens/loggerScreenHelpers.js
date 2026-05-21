import { reportClientErrorLater } from '../lib/errorReporting';

export const CONTACTS_STORAGE_KEY = 'log73.contacts';
export const SESSION_STORAGE_KEY = 'log73.session_id';
export const BACKEND_WS_INITIAL_RECONNECT_DELAY_MS = 2000;
export const BACKEND_WS_MAX_RECONNECT_DELAY_MS = 16000;
export const CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS = 2000;
export const CONTACTS_LOAD_MAX_RETRY_DELAY_MS = 16000;
export const CONTACTS_PAGE_SIZE = 200;
export const CONTACT_COMMIT_RETRY_DELAY_MS = 5000;
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

export function normalizeContact(contact) {
  const nextContact = { ...contact };
  if (nextContact._status === 'Committed') delete nextContact._client_id;
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
