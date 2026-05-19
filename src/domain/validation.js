import { parseFieldType } from './contactFields';

export function fieldValueLabel(field) {
  return field?.label ?? field?.name ?? 'Field';
}

export function validateExchangeField(field, value, radioMode = 'CW') {
  const label = fieldValueLabel(field);
  const normalizedValue = String(value ?? '').trim().toUpperCase();

  if (normalizedValue === '') {
    return { ok: false, error: `${label} is required.` };
  }

  const { kind } = parseFieldType(field?.type, radioMode);
  if (kind === 'RST') {
    const expectedLength = radioMode === 'CW' ? 3 : 2;
    if (!/^([1-5][1-9]{1,2})$/.test(normalizedValue) || normalizedValue.length !== expectedLength) {
      return { ok: false, error: `${label} must be a valid ${expectedLength}-digit RST.` };
    }
  } else if (kind === 'NUMERIC' && !/^\d+$/.test(normalizedValue)) {
    return { ok: false, error: `${label} must be numeric.` };
  }

  if ((field?.valid_values ?? []).length > 0) {
    const matches = field.valid_values.some((validValue) => String(validValue).toUpperCase() === normalizedValue);
    if (!matches) {
      return { ok: false, error: `${label} must be one of the configured values.` };
    }
  }

  if (field?.regex) {
    try {
      const regex = new RegExp(field.regex);
      if (!regex.test(normalizedValue)) {
        return { ok: false, error: `${label} is invalid.` };
      }
    } catch {
      return { ok: false, error: `${label} has an invalid validation pattern.` };
    }
  }

  return { ok: true, error: '' };
}
