const CATEGORY_OPERATOR = 'CATEGORY-OPERATOR';
const CATEGORY_TRANSMITTER = 'CATEGORY-TRANSMITTER';

function normalizedValue(value) {
  return String(value ?? '')
    .trim()
    .toUpperCase();
}

function logField(settings, name) {
  return (settings?.cabrillo?.log_fields ?? []).find(
    (field) => normalizedValue(field?.name) === name,
  );
}

function categoryValue(settings, log, name) {
  const fixedField = (settings?.cabrillo?.fixed_fields ?? []).find(
    (field) => normalizedValue(field?.name) === name,
  );
  if (fixedField) return normalizedValue(fixedField.value);

  const field = logField(settings, name);
  return normalizedValue(
    log?.contest_params?.[field?.name ?? name] ?? field?.default,
  );
}

export function cabrilloTransmitterPrompt(settings, log) {
  if (categoryValue(settings, log, CATEGORY_OPERATOR) !== 'MULTI-OP') {
    return null;
  }

  const transmitter = categoryValue(settings, log, CATEGORY_TRANSMITTER);
  if (transmitter === 'TWO') {
    return {
      kind: 'multi-two',
      question: 'Transmitter ID?',
      options: [
        { id: 0, label: 'One' },
        { id: 1, label: 'Two' },
      ],
    };
  }

  const transmitterField = logField(settings, CATEGORY_TRANSMITTER);
  if (
    transmitter === 'ONE' &&
    transmitterField?.multi_single_has_mult_transmitter === true
  ) {
    return {
      kind: 'multi-single',
      question: null,
      options: [
        { id: 0, label: 'Run Transmitter' },
        { id: 1, label: 'Mults Transmitter' },
      ],
    };
  }

  return null;
}

export function cabrilloTransmitterAdif(transmitterId) {
  return transmitterId === 0 || transmitterId === 1
    ? { APP_LOG73_TX_ID: transmitterId }
    : {};
}
