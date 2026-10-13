import assert from 'node:assert/strict';
import test from 'node:test';
import {
  contactWarnings,
  contactWarningsTitle,
  sanitizeLogCellUpdate,
} from './logWindowHelpers.js';

test('contactWarnings reads server diagnostics and ignores anything else', () => {
  assert.deepEqual(
    contactWarnings({
      meta: { warnings: [{ message: 'serial is required' }] },
    }),
    [{ message: 'serial is required' }],
  );
  assert.deepEqual(contactWarnings({ meta: {} }), []);
  assert.deepEqual(contactWarnings({ meta: { warnings: 'oops' } }), []);
  assert.deepEqual(contactWarnings(null), []);
});

test('contactWarningsTitle summarizes warning messages', () => {
  assert.equal(
    contactWarningsTitle({
      meta: {
        warnings: [
          { message: 'serial is required' },
          { message: 'band denied' },
        ],
      },
    }),
    'Needs attention: serial is required; band denied',
  );
  assert.equal(contactWarningsTitle({ meta: {} }), null);
  assert.equal(
    contactWarningsTitle({ meta: { warnings: [{ note: 'no message' }] } }),
    null,
  );
});

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
