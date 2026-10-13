import { readFileSync } from 'node:fs';

const FIXTURE_URL = new URL('./callsigns.json', import.meta.url);

export function loadCallsignFixture() {
  return JSON.parse(readFileSync(FIXTURE_URL, 'utf8'));
}
