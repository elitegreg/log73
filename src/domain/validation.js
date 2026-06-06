import { parseFieldType } from './contactFields.js';
import { modeIsCw } from './modes.js';

export function fieldValueLabel(field) {
  return field?.label ?? field?.name ?? 'Field';
}

function validateSingleValue(field, value, radioMode) {
  const label = fieldValueLabel(field);
  const trimmedValue = String(value ?? '').trim();
  const normalizedValue = trimmedValue.toUpperCase();

  const { kind } = parseFieldType(field?.type, radioMode);
  if (kind === 'RST') {
    const expectedLength = modeIsCw(radioMode) ? 3 : 2;
    if (
      !/^([1-5][1-9]{1,2})$/.test(normalizedValue) ||
      normalizedValue.length !== expectedLength
    ) {
      return {
        ok: false,
        error: `${label} must be a valid ${expectedLength}-digit RST.`,
      };
    }
  } else if (
    (kind === 'NUMERIC' || kind === 'SERIAL') &&
    !/^\d+$/.test(normalizedValue)
  ) {
    return { ok: false, error: `${label} must be numeric.` };
  }

  if ((field?.valid_values ?? []).length > 0) {
    const matches = field.valid_values.some(
      (validValue) => String(validValue).toUpperCase() === normalizedValue,
    );
    if (!matches) {
      return {
        ok: false,
        error: `${label} must be one of the configured values.`,
      };
    }
  }

  if (field?.regex) {
    try {
      const regex = new RegExp(field.regex);
      if (!regex.test(trimmedValue)) {
        return { ok: false, error: `${label} is invalid.` };
      }
    } catch {
      return {
        ok: false,
        error: `${label} has an invalid validation pattern.`,
      };
    }
  }

  return { ok: true, error: '' };
}

export function validateCallsign(value) {
  const normalizedValue = String(value ?? '')
    .trim()
    .toUpperCase();

  if (normalizedValue === '') {
    return { ok: false, error: 'Callsign is required.' };
  }

  if (!/^[A-Z0-9/?]+$/.test(normalizedValue)) {
    return {
      ok: false,
      error: "Callsign can only contain A-Z, 0-9, '/', and '?'.",
    };
  }

  if (normalizedValue.includes('?')) {
    return {
      ok: false,
      error: "Callsign cannot contain '?' when logging.",
    };
  }

  const slashCount = [...normalizedValue].filter(
    (character) => character === '/',
  ).length;
  if (slashCount > 1) {
    return { ok: false, error: 'Callsign can contain at most one slash.' };
  }

  if (normalizedValue.startsWith('/') || normalizedValue.endsWith('/')) {
    return {
      ok: false,
      error: 'Callsign slash must be in the middle, not at the ends.',
    };
  }

  const secondCharacter = normalizedValue[1] ?? '';
  const thirdCharacter = normalizedValue[2] ?? '';
  if (!/\d/.test(secondCharacter) && !/\d/.test(thirdCharacter)) {
    return {
      ok: false,
      error: 'Callsign must have a digit in the 2nd or 3rd character.',
    };
  }

  return { ok: true, error: '' };
}

export function validateConfiguredField(field, value, radioMode = 'CW') {
  const label = fieldValueLabel(field);
  const normalizedValue = String(value ?? '').trim();
  const multiline =
    String(field?.widget ?? '').toLowerCase() === 'textarea' ||
    Number.isInteger(field?.max_lines);

  if (normalizedValue === '') {
    if (field?.required === false) {
      return { ok: true, error: '' };
    }
    return { ok: false, error: `${label} is required.` };
  }

  if (multiline) {
    const lines = normalizedValue
      .replace(/\r\n/g, '\n')
      .split('\n')
      .filter((line) => line.trim() !== '');
    if (Number.isInteger(field?.max_lines) && lines.length > field.max_lines) {
      return {
        ok: false,
        error: `${label} must be at most ${field.max_lines} lines.`,
      };
    }
    for (const line of lines) {
      const result = validateSingleValue(field, line, radioMode);
      if (!result.ok) {
        return result;
      }
    }
    return { ok: true, error: '' };
  }

  return validateSingleValue(field, normalizedValue, radioMode);
}

export function validateExchangeField(field, value, radioMode = 'CW') {
  return validateConfiguredField(field, value, radioMode);
}
