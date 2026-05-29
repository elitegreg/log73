const POSSIBLE_DUPE_FIELDS = new Set(['CALL', 'BAND', 'MODE']);

function jsonString(value) {
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return null;
}

function normalizedFieldValue(field, value) {
  const normalized = String(value ?? '')
    .trim()
    .toUpperCase();
  if (field.toUpperCase() === 'CALL') {
    return normalized.split('/')[0];
  }
  return normalized;
}

function contactFieldValue(settings, contact, field) {
  const directValue = jsonString(contact?.[field]);
  if (directValue !== null) return normalizedFieldValue(field, directValue);

  if (field.toUpperCase() === 'CALL') {
    const legacyCallValue = jsonString(contact?.Call);
    if (legacyCallValue !== null) {
      return normalizedFieldValue(field, legacyCallValue);
    }
  }

  const mappedField = settings?.qso_column_fields?.[field];
  const mappedValue = mappedField ? jsonString(contact?.[mappedField]) : null;
  if (mappedValue !== null) return normalizedFieldValue(field, mappedValue);

  return '';
}

function keyForFields(settings, contact, fields) {
  return fields
    .map((field) => contactFieldValue(settings, contact, field))
    .join('|');
}

function possibleDupeFields(settings) {
  return (settings?.dupe_key ?? []).filter((field) =>
    POSSIBLE_DUPE_FIELDS.has(field.toUpperCase()),
  );
}

export function dupeAlertText(settings, currentContact, historicContacts) {
  const dupeFields = settings?.dupe_key ?? [];
  const possibleFields = possibleDupeFields(settings);
  if (dupeFields.length === 0 || possibleFields.length === 0) return '';

  const currentCallsign = contactFieldValue(settings, currentContact, 'CALL');
  if (!currentCallsign) return '';

  const currentPossibleKey = keyForFields(settings, currentContact, possibleFields);
  const currentDupeKey = keyForFields(settings, currentContact, dupeFields);
  let alertText = '';

  for (const historicContact of historicContacts ?? []) {
    const historicCallsign = contactFieldValue(settings, historicContact, 'CALL');
    if (historicCallsign !== currentCallsign) break;

    if (
      keyForFields(settings, historicContact, possibleFields) !==
      currentPossibleKey
    ) {
      continue;
    }

    if (keyForFields(settings, historicContact, dupeFields) === currentDupeKey) {
      return 'Dupe';
    }

    alertText = 'Possible Dupe';
  }

  return alertText;
}
