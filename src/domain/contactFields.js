export function parseFieldType(type = '', radioMode = 'CW') {
  const [rawKind = 'STRING', length = '8'] = type.split(':');
  const kind = rawKind.toUpperCase();
  const maxLength =
    kind === 'RST'
      ? radioMode === 'CW'
        ? 3
        : 2
      : Number.parseInt(length, 10) || 8;
  return { kind, maxLength };
}

export function sanitizeRST(value, radioMode = 'CW') {
  const maxLength = radioMode === 'CW' ? 3 : 2;
  let nextValue = value.replace(/[^1-9]/g, '').slice(0, maxLength);

  while (nextValue.length > 0 && !/^[1-5]$/.test(nextValue[0])) {
    nextValue = nextValue.slice(1);
  }

  return nextValue;
}

export function sanitizeCallsign(value) {
  return value.toUpperCase().slice(0, 12);
}

export function sanitizeExchangeValue(field, value, radioMode = 'CW') {
  const { kind, maxLength } = parseFieldType(field?.type, radioMode);
  let nextValue = String(value).slice(0, maxLength);

  if (kind === 'RST') {
    nextValue = sanitizeRST(nextValue, radioMode);
  } else if (kind === 'NUMERIC') {
    nextValue = nextValue.replace(/\D/g, '');
  } else {
    nextValue = nextValue.toUpperCase();
  }

  return nextValue;
}

export function fieldDefault(field, radioMode, contestParams = {}) {
  const sourceParam = field?.source_param;
  const rawValue = sourceParam ? contestParams?.[sourceParam] : field?.default;

  if (rawValue === undefined || rawValue === null) {
    return '';
  }

  const value = String(rawValue);
  return parseFieldType(field.type, radioMode).kind === 'RST'
    ? sanitizeRST(value, radioMode)
    : value.toUpperCase();
}
