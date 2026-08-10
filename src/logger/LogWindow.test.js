import assert from 'node:assert/strict';
import test from 'node:test';
import { sanitizeLogCellUpdate } from './logWindowHelpers.js';

test('inline RST edits preserve a CW RST when the active radio is SSB', () => {
  const exchangeFields = [
    {
      id: 'rst-received',
      label: 'RST(r)',
      input: { kind: 'rst' },
      adif: 'RST_RCVD',
    },
  ];
  const column = { field: 'RST_RCVD' };

  assert.equal(
    sanitizeLogCellUpdate(exchangeFields, column, '599', 'CW'),
    '599',
  );
  assert.equal(
    sanitizeLogCellUpdate(exchangeFields, column, '599', 'SSB'),
    '59',
  );
});
