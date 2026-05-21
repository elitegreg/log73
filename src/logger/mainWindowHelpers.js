export const MODE_OPTIONS = ['CW', 'SSB', 'FM'];
export const CW_WPM_STORAGE_KEY = 'log73.cw_wpm';
export const DEFAULT_CW_LABELS = {
  run: Array.from({ length: 12 }, (_, index) => ({ key: `F${index + 1}`, label: '-' })),
  's&p': Array.from({ length: 12 }, (_, index) => ({ key: `F${index + 1}`, label: '-' })),
};
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
    (settings?.exchange ?? []).map((field) => [field.name, fieldDefault(field, radioMode, contestParams)]),
  );
}

export function formatFrequency(frequencyHz) {
  return Math.round(frequencyHz / 1000);
}

export function isFrequencyInput(value) {
  return /^\d+(\.\d+)?$/.test(value.trim());
}

export function bandForFrequency(frequencyHz) {
  return AMATEUR_BANDS.find((band) => frequencyHz >= band.lowerHz && frequencyHz <= band.upperHz);
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

import { fieldDefault } from '../domain/contactFields';
