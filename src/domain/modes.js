export const LOGGER_MODE_OPTIONS = [
  'CW',
  'CW-R',
  'SSB',
  'FM',
  'AM',
  'FT8',
  'JT65',
  'JT9',
  'MFSK',
  'PSK',
  'RTTY',
];

const SELECTABLE_MODES = new Set(LOGGER_MODE_OPTIONS);
const CW_MODES = new Set(['CW', 'CW-R']);
const PHONE_MODES = new Set(['SSB', 'FM', 'AM']);

export function normalizeLoggerMode(mode) {
  return String(mode ?? '')
    .trim()
    .toUpperCase();
}

export function isSelectableMode(mode) {
  return SELECTABLE_MODES.has(normalizeLoggerMode(mode));
}

export function modeIsCw(mode) {
  return CW_MODES.has(normalizeLoggerMode(mode));
}

export function modeIsPhone(mode) {
  return PHONE_MODES.has(normalizeLoggerMode(mode));
}

export function adifModeForLoggerMode(mode) {
  const normalizedMode = normalizeLoggerMode(mode);
  return modeIsCw(normalizedMode) ? 'CW' : normalizedMode;
}
