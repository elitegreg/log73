import {
  fieldDefault,
  parseFieldType,
  sanitizeExchangeValue,
} from '../domain/contactFields.js';
import { splitCallsign } from '../domain/dxcc.js';
import {
  LOGGER_MODE_OPTIONS,
  adifModeForLoggerMode,
  isSelectableMode,
  modeIsCw,
  modeIsPhone,
  normalizeLoggerMode,
} from '../domain/modes.js';
import {
  actionFromTemplate,
  messageActionForConfig,
} from '../domain/messages.js';

export { adifModeForLoggerMode, isSelectableMode, modeIsCw, modeIsPhone };

export const MODE_OPTIONS = LOGGER_MODE_OPTIONS;
export const CW_WPM_STORAGE_KEY = 'log73.cw_wpm';
export const ESM_ENABLED_STORAGE_KEY = 'log73.esm_enabled';
export const BAND_MAP_ENABLED_STORAGE_KEY = 'log73.band_map_enabled';
export const DEFAULT_MESSAGE_LABELS = {
  run: Array.from({ length: 12 }, (_, index) => ({
    key: `F${index + 1}`,
    label: '-',
  })),
  's&p': Array.from({ length: 12 }, (_, index) => ({
    key: `F${index + 1}`,
    label: '-',
  })),
};

export const DEFAULT_CW_WPM = 20;
export const CW_WPM_MIN = 5;
export const CW_WPM_MAX = 60;
export const CW_WPM_STEP = 1;
export const DEFAULT_RADIO_FREQUENCY_HZ = 14000000;
export const SUPERCHECKPARTIAL_MIN_QUERY_LENGTH = 3;
export const CALLSIGN_LOOKUP_DEBOUNCE_MS = 500;
export const CW_ACTIVE_TIMEOUT_WIKEYER_MS = 30000;
export const CW_ACTIVE_TIMEOUT_CAT_MS = 30000;
export const CW_ACTIVE_TIMEOUT_NONE_MS = 500;
export const CW_REPEAT_DELAY_MS = 2000;
export const DEFAULT_CW_TUNING_INCREMENT_HZ = 20;
export const DEFAULT_SSB_TUNING_INCREMENT_HZ = 100;
export const FUNCTION_KEY_PATTERN = /^F([1-9]|1[0-2])$/;
export const HZ_PER_KHZ = 1000;
export const EPOCH_MS_PER_SECOND = 1000;
export const CALLSIGN_FIELD_WIDTH_CHARS = 13;
export const CW_DIGITAL_CALLSIGN_CLEAR_THRESHOLD_HZ = 100;
export const PHONE_CALLSIGN_CLEAR_THRESHOLD_HZ = 200;

const PHONE_MODES = new Set(['SSB', 'FM', 'AM']);

export function exchangeDefaults(settings, radioMode, contestParams = {}) {
  return Object.fromEntries(
    (settings?.exchange ?? []).map((field) => [
      field.name,
      fieldDefault(field, radioMode, contestParams),
    ]),
  );
}

function normalizedAutofillCallsign(value) {
  return String(value ?? '')
    .trim()
    .toUpperCase();
}

function contactAutofillCallsign(contact) {
  return normalizedAutofillCallsign(contact?.adif?.CALL ?? '');
}

function contactExchangeRawValue(contact, field) {
  const adif = contact?.adif ?? {};
  for (const key of [field?.adif, field?.name].filter(Boolean)) {
    if (Object.prototype.hasOwnProperty.call(adif, key)) {
      return adif[key];
    }
  }
  return undefined;
}

function shouldReplaceWithAutofillValue(field, currentValue, defaultValue) {
  const currentText = String(currentValue ?? '');
  if (currentText.trim() === '') return true;
  if (field?.fixed === true) return false;
  return currentText === String(defaultValue ?? '');
}

function isSerialExchangeField(field, radioMode) {
  return parseFieldType(field?.type, radioMode).kind === 'SERIAL';
}

export function previousContactExchangeAutofill({
  settings,
  contacts = [],
  callsign,
  exchangeValues = {},
  radioMode = 'CW',
  contestParams = {},
} = {}) {
  const defaults = exchangeDefaults(settings, radioMode, contestParams);
  const values = { ...defaults, ...(exchangeValues ?? {}) };
  const normalizedCallsign = normalizedAutofillCallsign(callsign);
  const matchedContact = normalizedCallsign
    ? (contacts ?? []).find(
        (contact) => contactAutofillCallsign(contact) === normalizedCallsign,
      )
    : null;

  if (!matchedContact) {
    return {
      matchedContact: null,
      changed: false,
      copiedFields: [],
      values,
    };
  }

  const nextValues = { ...values };
  const copiedFields = [];

  for (const field of settings?.exchange ?? []) {
    if (isSerialExchangeField(field, radioMode)) continue;

    const rawValue = contactExchangeRawValue(matchedContact, field);
    if (rawValue === undefined || rawValue === null) continue;

    const previousValue = sanitizeExchangeValue(field, rawValue, radioMode);
    if (String(previousValue).trim() === '') continue;

    const currentValue = nextValues[field.name] ?? '';
    if (
      !shouldReplaceWithAutofillValue(
        field,
        currentValue,
        defaults[field.name] ?? '',
      )
    ) {
      continue;
    }

    if (String(currentValue ?? '') === previousValue) continue;

    nextValues[field.name] = previousValue;
    copiedFields.push(field.name);
  }

  return {
    matchedContact,
    changed: copiedFields.length > 0,
    copiedFields,
    values: nextValues,
  };
}

export function shouldAdvanceFromCallsignAutofill({
  esmEnabled,
  operatingMode,
  autofillResult,
  hasEditableExchangeField,
} = {}) {
  return Boolean(
    esmEnabled &&
      operatingMode !== 'Run' &&
      autofillResult?.matchedContact &&
      hasEditableExchangeField,
  );
}

export function cwActiveTimeoutMs(cwKeyerType) {
  switch (cwKeyerType) {
    case 'winkeyer':
      return CW_ACTIVE_TIMEOUT_WIKEYER_MS;
    case 'cat':
    case 'serial':
      return CW_ACTIVE_TIMEOUT_CAT_MS;
    default:
      return CW_ACTIVE_TIMEOUT_NONE_MS;
  }
}

export function formatFrequency(frequencyHz) {
  return Math.round(frequencyHz / HZ_PER_KHZ);
}

export function callsignClearThresholdHz(mode) {
  return PHONE_MODES.has(normalizeLoggerMode(mode))
    ? PHONE_CALLSIGN_CLEAR_THRESHOLD_HZ
    : CW_DIGITAL_CALLSIGN_CLEAR_THRESHOLD_HZ;
}

export function loggerFrequencyChangeAction({
  previousFrequencyHz,
  nextFrequencyHz,
  thresholdHz,
  pendingBandMapTuneFrequencyHz = null,
} = {}) {
  if (!Number.isFinite(previousFrequencyHz) || !Number.isFinite(nextFrequencyHz)) {
    return 'none';
  }
  if (Number.isFinite(pendingBandMapTuneFrequencyHz)) {
    return Math.abs(nextFrequencyHz - pendingBandMapTuneFrequencyHz) < thresholdHz
      ? 'clear-pending-bandmap-tune'
      : 'none';
  }
  return Math.abs(nextFrequencyHz - previousFrequencyHz) >= thresholdHz
    ? 'clear-logger'
    : 'none';
}

export function normalizedContactFrequencyHz(value) {
  const frequency = Number.parseFloat(String(value ?? ''));
  if (!Number.isFinite(frequency) || frequency <= 0) return 0;
  return Math.round(
    Math.abs(frequency) < 1000000 ? frequency * 1000000 : frequency,
  );
}

export function isFrequencyInput(value) {
  return /^\d+(\.\d+)?$/.test(value.trim());
}

export function bandForFrequency(frequencyHz, bands = []) {
  return bands.find(
    (band) => frequencyHz >= band.lowerHz && frequencyHz <= band.upperHz,
  );
}

export function bandByName(bands = [], name) {
  return bands.find((band) => band.name === name);
}

export function createContactId(date, callSign) {
  if (window.crypto?.randomUUID) return window.crypto.randomUUID();
  return `${date.getTime()}-${callSign}-${Math.random().toString(36).slice(2)}`;
}

export function messageButtonLabel(label, stationCallsign) {
  return String(label ?? '').replaceAll('{STATION_CALLSIGN}', stationCallsign);
}

export function createMessageRequestId() {
  if (window.crypto?.randomUUID) return window.crypto.randomUUID();
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function isEmptyMessageButton(button) {
  return String(button?.label ?? '').trim() === '-';
}

export function messageButtonIsSendable(button) {
  return !isEmptyMessageButton(button);
}

export function cwActionFromTemplate(template) {
  return actionFromTemplate(template);
}

export function messageActionForRadioMode(
  cwConfig,
  voiceConfig,
  mode,
  key,
  radioMode,
) {
  const config = modeIsPhone(radioMode) ? voiceConfig : cwConfig;
  return cwActionForMessage(config, mode, key);
}

export function cwActionForMessage(config, mode, key) {
  return messageActionForConfig(config, mode, key);
}

export function availableModeOptions(settings = {}) {
  return settings?.mode_catalog?.length > 0
    ? settings.mode_catalog
    : MODE_OPTIONS;
}

export function typedModeFromCallsignInput(value, settings) {
  const normalizedValue = normalizeLoggerMode(value);
  if (!normalizedValue) return null;

  const modeAliases = {
    CWR: 'CW-R',
  };
  const candidateValue = modeAliases[normalizedValue] ?? normalizedValue;

  return (
    availableModeOptions(settings).find((mode) => mode === candidateValue) ??
    null
  );
}

export function callsignHasQuery(value) {
  return String(value ?? '')
    .trim()
    .includes('?');
}

export function shouldBlockEsmCallEnter(callsign, callsignIsValid) {
  const normalizedCallsign = String(callsign ?? '').trim();
  if (normalizedCallsign === '') return false;
  return !callsignIsValid;
}

export function nextCwWpm(currentWpm, delta) {
  const normalizedCurrentWpm = Number.isFinite(currentWpm)
    ? currentWpm
    : DEFAULT_CW_WPM;
  return Math.min(
    CW_WPM_MAX,
    Math.max(CW_WPM_MIN, normalizedCurrentWpm + delta),
  );
}

export function isPageUpKey(event) {
  return event?.key === 'PageUp' || event?.key === 'Prior';
}

export function isPageDownKey(event) {
  return event?.key === 'PageDown' || event?.key === 'Next';
}

function normalizedPositiveHz(value, fallbackHz) {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) return fallbackHz;
  return parsed;
}

export function tuningIncrementHzForMode(radio, mode) {
  const defaultIncrementHz = modeIsCw(mode)
    ? DEFAULT_CW_TUNING_INCREMENT_HZ
    : DEFAULT_SSB_TUNING_INCREMENT_HZ;
  const configuredIncrementHz = modeIsCw(mode)
    ? radio?.cw_tuning_increment_hz
    : radio?.ssb_tuning_increment_hz;
  return normalizedPositiveHz(configuredIncrementHz, defaultIncrementHz);
}

export function steppedFrequencyHz(frequencyHz, deltaHz) {
  const nextFrequencyHz = Math.round(Number(frequencyHz) + Number(deltaHz));
  if (!Number.isFinite(nextFrequencyHz)) return 1;
  return Math.max(1, nextFrequencyHz);
}

export function correctedEsmCallsignText(
  exchangeSentCallsign,
  correctedCallsign,
) {
  const normalizedSentCallsign = String(exchangeSentCallsign ?? '')
    .trim()
    .toUpperCase();
  const normalizedCorrectedCallsign = String(correctedCallsign ?? '')
    .trim()
    .toUpperCase();

  if (
    !normalizedSentCallsign ||
    !normalizedCorrectedCallsign ||
    normalizedSentCallsign === normalizedCorrectedCallsign
  ) {
    return '';
  }

  const sentParts = splitCallsign(normalizedSentCallsign);
  const correctedParts = splitCallsign(normalizedCorrectedCallsign);
  if (!sentParts || !correctedParts) {
    return normalizedCorrectedCallsign;
  }

  if (
    sentParts.prefix === correctedParts.prefix &&
    sentParts.number === correctedParts.number
  ) {
    return correctedParts.suffix || normalizedCorrectedCallsign;
  }

  return normalizedCorrectedCallsign;
}

export function esmEnterAction({
  esmEnabled,
  operatingMode,
  callsign,
  exchangeValid,
  exchangeSentCallsign,
  runCallsignAttempt,
}) {
  if (!esmEnabled) {
    return {
      keys: [],
      correctionText: '',
      shouldLog: false,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: exchangeSentCallsign ?? '',
    };
  }

  const normalizedCallsign = String(callsign ?? '')
    .trim()
    .toUpperCase();
  const normalizedRunCallsignAttempt = String(runCallsignAttempt ?? '')
    .trim()
    .toUpperCase();
  const normalizedExchangeSentCallsign = String(exchangeSentCallsign ?? '')
    .trim()
    .toUpperCase();
  const isRunMode = operatingMode === 'Run';

  if (!normalizedCallsign) {
    return {
      keys: [isRunMode ? 'F1' : 'F4'],
      correctionText: '',
      shouldLog: false,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: '',
    };
  }

  if (!exchangeValid) {
    if (!isRunMode) {
      return {
        keys: ['F4'],
        correctionText: '',
        shouldLog: false,
        nextRunCallsignAttempt: '',
        nextExchangeSentCallsign: normalizedExchangeSentCallsign,
      };
    }

    const isRepeatCall = normalizedRunCallsignAttempt === normalizedCallsign;
    if (isRepeatCall) {
      return {
        keys: ['F8'],
        correctionText: '',
        shouldLog: false,
        nextRunCallsignAttempt: normalizedRunCallsignAttempt,
        nextExchangeSentCallsign: normalizedExchangeSentCallsign,
      };
    }

    return {
      keys: ['F5', 'F2'],
      correctionText: '',
      shouldLog: false,
      nextRunCallsignAttempt: normalizedCallsign,
      nextExchangeSentCallsign: normalizedCallsign,
    };
  }

  if (isRunMode) {
    const exchangeAlreadySentForCallsign =
      normalizedExchangeSentCallsign === normalizedCallsign;
    if (exchangeAlreadySentForCallsign) {
      return {
        keys: ['F3'],
        correctionText: '',
        shouldLog: true,
        nextRunCallsignAttempt: normalizedCallsign,
        nextExchangeSentCallsign: normalizedCallsign,
      };
    }

    const correctionText = correctedEsmCallsignText(
      normalizedExchangeSentCallsign,
      normalizedCallsign,
    );
    if (correctionText) {
      return {
        keys: ['F3'],
        correctionText,
        shouldLog: true,
        nextRunCallsignAttempt: normalizedCallsign,
        nextExchangeSentCallsign: normalizedCallsign,
      };
    }

    return {
      keys: ['F5', 'F2'],
      correctionText: '',
      shouldLog: false,
      nextRunCallsignAttempt: normalizedCallsign,
      nextExchangeSentCallsign: normalizedCallsign,
    };
  }

  const exchangeAlreadySentForCallsign =
    normalizedExchangeSentCallsign === normalizedCallsign;
  if (exchangeAlreadySentForCallsign) {
    return {
      keys: [],
      correctionText: '',
      shouldLog: true,
      nextRunCallsignAttempt: '',
      nextExchangeSentCallsign: normalizedCallsign,
    };
  }

  return {
    keys: ['F2'],
    correctionText: '',
    shouldLog: true,
    nextRunCallsignAttempt: '',
    nextExchangeSentCallsign: normalizedCallsign,
  };
}
