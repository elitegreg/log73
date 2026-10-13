import { dupeIdentity } from './callsign.js';
import { normalizeLoggerMode } from './modes.js';

const POSSIBLE_DUPE_FIELDS = new Set(['CALL', 'BAND', 'MODE', 'MODE_CLASS']);

function jsonString(value) {
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return null;
}

// MODE grouping for duplicate keys. Mirrors backend `dupe_mode_class`:
// CW/CW-R stay CW, SSB and FM stay distinct, everything else collapses to
// DATA (so FT8 and RTTY match). Returns '' for a missing MODE.
export function dupeModeKey(mode) {
  const normalized = normalizeLoggerMode(mode);
  if (normalized === '') return '';
  if (normalized === 'CW' || normalized === 'CW-R') return 'CW';
  if (normalized === 'SSB') return 'SSB';
  if (normalized === 'FM') return 'FM';
  return 'DATA';
}

// MODE_CLASS derivation for duplicate keys. Mirrors the MODE_CLASS branch of
// backend `field_value`: CW/CW-R stay CW, SSB/FM/AM share PHONE, and every
// other mode keeps its normalized name. Unknown modes pass through instead of
// collapsing to '' so unrelated modes never false-match. Returns '' only for
// a missing MODE.
export function modeClassKey(mode) {
  const normalized = normalizeLoggerMode(mode);
  if (normalized === 'CW' || normalized === 'CW-R') return 'CW';
  if (normalized === 'SSB' || normalized === 'FM' || normalized === 'AM') {
    return 'PHONE';
  }
  return normalized;
}

function normalizedFieldValue(field, value) {
  const normalized = normalizeLoggerMode(value);
  if (field.toUpperCase() === 'CALL') {
    return dupeIdentity(normalized);
  }
  return normalized;
}

function contactFieldValue(settings, contact, field) {
  const adif = contact?.adif ?? {};
  // MODE_CLASS is always derived from MODE (a stored MODE_CLASS value is
  // ignored), matching the backend reference.
  if (field.toUpperCase() === 'MODE_CLASS') {
    return modeClassKey(jsonString(adif?.MODE));
  }
  if (field.toUpperCase() === 'MODE') {
    return dupeModeKey(jsonString(adif?.MODE));
  }
  const directValue = jsonString(adif?.[field]);
  if (directValue !== null) return normalizedFieldValue(field, directValue);

  return '';
}

function keyForFields(settings, contact, fields) {
  return fields
    .map((field) => contactFieldValue(settings, contact, field))
    .join('|');
}

function possibleDupeFields(settings) {
  return (settings?.scoring?.dupe_key ?? []).filter((field) =>
    POSSIBLE_DUPE_FIELDS.has(field.toUpperCase()),
  );
}

// Duplicate key for a contact under the configured dupe key. Mirrors backend
// `dupe_key_for`: same `|`-joined key string, same per-field values.
export function dupeKey(settings, contact) {
  const dupeFields = settings?.scoring?.dupe_key ?? [];
  return keyForFields(settings, contact, dupeFields);
}

export function dupeAlertText(settings, currentContact, historicContacts) {
  const dupeFields = settings?.scoring?.dupe_key ?? [];
  const possibleFields = possibleDupeFields(settings);
  if (dupeFields.length === 0 || possibleFields.length === 0) return '';

  const currentCallsign = contactFieldValue(settings, currentContact, 'CALL');
  if (!currentCallsign) return '';

  const currentPossibleKey = keyForFields(
    settings,
    currentContact,
    possibleFields,
  );
  const currentDupeKey = dupeKey(settings, currentContact);
  let alertText = '';

  for (const historicContact of historicContacts ?? []) {
    const historicCallsign = contactFieldValue(
      settings,
      historicContact,
      'CALL',
    );
    if (historicCallsign !== currentCallsign) continue;

    if (
      keyForFields(settings, historicContact, possibleFields) !==
      currentPossibleKey
    ) {
      continue;
    }

    if (dupeKey(settings, historicContact) === currentDupeKey) {
      return 'Dupe';
    }

    alertText = 'Possible Dupe';
  }

  return alertText;
}

export function dupeAlertTextForAdif(settings, currentAdif, historicContacts) {
  return dupeAlertText(settings, { adif: currentAdif }, historicContacts);
}
