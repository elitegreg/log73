import { readFileSync } from 'node:fs';

const CALLSIGNS_URL = new URL('./callsigns.json', import.meta.url);
const DUPE_KEYS_URL = new URL('./dupe-keys.json', import.meta.url);

export function loadCallsignFixture() {
  return JSON.parse(readFileSync(CALLSIGNS_URL, 'utf8'));
}

export function loadDupeKeysFixture() {
  return JSON.parse(readFileSync(DUPE_KEYS_URL, 'utf8'));
}
