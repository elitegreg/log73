import {
  sanitizeCallsign,
  sanitizeExchangeValue,
} from '../domain/contactFields.js';

export function sanitizeLogCellUpdate(exchangeFields, column, value, radioMode) {
  const exchangeField = (exchangeFields ?? []).find(
    (field) => field.name === column,
  );
  if (exchangeField)
    return sanitizeExchangeValue(exchangeField, value, radioMode);
  if (column === 'Call') return sanitizeCallsign(value);
  if (column === 'Mode') return String(value).toUpperCase();
  return String(value).toUpperCase();
}
