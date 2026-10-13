import {
  sanitizeCallsign,
  sanitizeExchangeValue,
} from '../domain/contactFields.js';

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
