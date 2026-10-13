import {
  sanitizeCallsign,
  sanitizeExchangeValue,
} from '../domain/contactFields.js';

// Server-computed diagnostic warnings for a log entry. Returns the warnings
// array, or [] when the entry carries none. Entries shape as { meta, adif }.
export function contactWarnings(entry) {
  const warnings = entry?.meta?.warnings;
  return Array.isArray(warnings) ? warnings : [];
}

// Human-readable warning detail for a warned row, or null when there is
// nothing to show.
export function contactWarningsTitle(entry) {
  const messages = contactWarnings(entry)
    .map((warning) => warning?.message)
    .filter((message) => typeof message === 'string' && message !== '');
  if (messages.length === 0) return null;
  return `Needs attention: ${messages.join('; ')}`;
}

export function sanitizeLogCellUpdate(
  exchangeFields,
  column,
  value,
  radioMode,
) {
  const exchangeField = (exchangeFields ?? []).find(
    (field) => field.adif === column.field,
  );
  if (exchangeField)
    return sanitizeExchangeValue(exchangeField, value, radioMode);
  if (column.field === 'CALL') return sanitizeCallsign(value);
  if (column.field === 'MODE') return String(value).toUpperCase();
  return String(value).toUpperCase();
}
