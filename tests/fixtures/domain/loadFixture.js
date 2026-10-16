import { readFileSync } from 'node:fs';

const CALLSIGNS_URL = new URL('./callsigns.json', import.meta.url);
const DUPE_KEYS_URL = new URL('./dupe-keys.json', import.meta.url);
const CONDITIONS_URL = new URL('./conditions.json', import.meta.url);

export function loadCallsignFixture() {
  return JSON.parse(readFileSync(CALLSIGNS_URL, 'utf8'));
}

export function loadDupeKeysFixture() {
  return JSON.parse(readFileSync(DUPE_KEYS_URL, 'utf8'));
}

export function loadConditionsFixture() {
  return JSON.parse(readFileSync(CONDITIONS_URL, 'utf8'));
}

export function loadModesFixture() {
  return JSON.parse(
    readFileSync(new URL('./modes.json', import.meta.url), 'utf8'),
  );
}

export function loadMessageModesFixture() {
  return JSON.parse(
    readFileSync(new URL('./message-modes.json', import.meta.url), 'utf8'),
  );
}

export function loadMessageEntriesFixture() {
  return JSON.parse(
    readFileSync(new URL('./message-entries.json', import.meta.url), 'utf8'),
  );
}
