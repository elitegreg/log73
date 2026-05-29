import { fieldDefault } from '../domain/contactFields.js';
import {
  LOGGER_MODE_OPTIONS,
  normalizeLoggerMode,
} from '../domain/modes.js';

export {
  adifModeForLoggerMode,
  isSelectableMode,
  modeIsCw,
} from '../domain/modes.js';

export const MODE_OPTIONS = LOGGER_MODE_OPTIONS;
export const CW_WPM_STORAGE_KEY = 'log73.cw_wpm';
export const DEFAULT_CW_LABELS = {
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
export const FUNCTION_KEY_PATTERN = /^F([1-9]|1[0-2])$/;
export const HZ_PER_KHZ = 1000;
export const EPOCH_MS_PER_SECOND = 1000;
export const CALLSIGN_FIELD_WIDTH_CHARS = 13;

const AMATEUR_BANDS = [
  { meters: 160, name: '160m', lowerHz: 1800000, upperHz: 2000000 },
  { meters: 80, name: '80m', lowerHz: 3500000, upperHz: 4000000 },
  { meters: 60, name: '60m', lowerHz: 5330500, upperHz: 5406500 },
  { meters: 40, name: '40m', lowerHz: 7000000, upperHz: 7300000 },
  { meters: 30, name: '30m', lowerHz: 10100000, upperHz: 10150000 },
  { meters: 20, name: '20m', lowerHz: 14000000, upperHz: 14350000 },
  { meters: 17, name: '17m', lowerHz: 18068000, upperHz: 18168000 },
  { meters: 15, name: '15m', lowerHz: 21000000, upperHz: 21450000 },
  { meters: 12, name: '12m', lowerHz: 24890000, upperHz: 24990000 },
  { meters: 10, name: '10m', lowerHz: 28000000, upperHz: 29700000 },
  { meters: 6, name: '6m', lowerHz: 50000000, upperHz: 54000000 },
  { meters: 2, name: '2m', lowerHz: 144000000, upperHz: 148000000 },
];

export function exchangeDefaults(settings, radioMode, contestParams = {}) {
  return Object.fromEntries(
    (settings?.exchange ?? []).map((field) => [
      field.name,
      fieldDefault(field, radioMode, contestParams),
    ]),
  );
}

export function formatFrequency(frequencyHz) {
  return Math.round(frequencyHz / HZ_PER_KHZ);
}

export function isFrequencyInput(value) {
  return /^\d+(\.\d+)?$/.test(value.trim());
}

export function bandForFrequency(frequencyHz) {
  return AMATEUR_BANDS.find(
    (band) => frequencyHz >= band.lowerHz && frequencyHz <= band.upperHz,
  );
}

export function bandByMeters(meters) {
  return AMATEUR_BANDS.find((band) => band.meters === meters);
}

export function createContactId(date, callSign) {
  if (window.crypto?.randomUUID) return window.crypto.randomUUID();
  return `${date.getTime()}-${callSign}-${Math.random().toString(36).slice(2)}`;
}

export function cwButtonLabel(label, stationCallsign) {
  return String(label ?? '').replaceAll('{STATION_CALLSIGN}', stationCallsign);
}

export function createCwRequestId() {
  if (window.crypto?.randomUUID) return window.crypto.randomUUID();
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function isEmptyCwButton(button) {
  return String(button?.label ?? '').trim() === '-';
}

export function availableModeOptions() {
  return MODE_OPTIONS;
}

export function typedModeFromCallsignInput(value, settings) {
  const normalizedValue = normalizeLoggerMode(value);
  if (!normalizedValue) return null;

  return (
    availableModeOptions(settings).find((mode) => mode === normalizedValue) ??
    null
  );
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
