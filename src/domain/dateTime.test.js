import assert from 'node:assert/strict';
import test from 'node:test';
import {
  epochFromLegacyQsoDateTime,
  formatUtcDateTime,
  parseUtcDateTime,
} from './dateTime.js';

test('date helpers parse and format UTC timestamps', () => {
  assert.equal(
    epochFromLegacyQsoDateTime({
      adif: { QSO_DATE: '20231114', TIME_ON: '221523' },
    }),
    1700000123,
  );
  assert.equal(formatUtcDateTime(1700000123), '2023-11-14 22:15:23');
  assert.equal(parseUtcDateTime('2023-11-14 22:15:23'), 1700000123);
  assert.equal(parseUtcDateTime('2023-02-29 22:15:23'), null);
});
