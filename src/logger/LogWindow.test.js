import assert from 'node:assert/strict';
import test from 'node:test';
import { sanitizeLogCellUpdate } from './logWindowHelpers.js';

test('inline RST edits preserve a CW RST when the active radio is SSB', () => {
  const exchangeFields = [
    {
      name: 'RST(r)',
      type: 'RST',
      adif: 'RST_RCVD',
    },
  ];

  assert.equal(
    sanitizeLogCellUpdate(exchangeFields, 'RST(r)', '599', 'CW'),
    '599',
  );
  assert.equal(
    sanitizeLogCellUpdate(exchangeFields, 'RST(r)', '599', 'SSB'),
    '59',
  );
});
