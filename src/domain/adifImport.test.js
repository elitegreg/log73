import assert from 'node:assert/strict';
import test from 'node:test';
import {
  adifFieldOptionLabel,
  adifFieldOptions,
  fixedValueMappingErrors,
  parseFirstAdifRecord,
} from './adifImport.js';

test('parseFirstAdifRecord skips header and reads first QSO', () => {
  const record = parseFirstAdifRecord(
    'Log73\n<ADIF_VER:5>3.1.0<EOH>\n<CALL:4>W1AW<BAND:3>20m<EOR>',
  );

  assert.deepEqual(record.fields, {
    CALL: 'W1AW',
    BAND: '20m',
  });
});

test('parseFirstAdifRecord handles ADIF without EOH', () => {
  const record = parseFirstAdifRecord('<CALL:4>W1AW<SRX_STRING:2>NC<EOR>');

  assert.deepEqual(record.fields, {
    CALL: 'W1AW',
    SRX_STRING: 'NC',
  });
});

test('adifFieldOptions sorts fields and labels examples', () => {
  const options = adifFieldOptions({ SRX_STRING: 'NC', CALL: 'W1AW' });

  assert.deepEqual(
    options.map((option) => option.name),
    ['CALL', 'SRX_STRING'],
  );
  assert.equal(adifFieldOptionLabel(options[1]), "SRX_STRING (e.g. 'NC')");
});

test('fixedValueMappingErrors rejects blank fixed set values', () => {
  const fields = [
    { name: 'Class', adif: 'SRX_STRING' },
    { name: 'Section', adif: 'ARRL_SECT' },
  ];
  const mappings = {
    SRX_STRING: { kind: 'fixed_value', value: '   ' },
    ARRL_SECT: { kind: 'fixed_value', value: 'DX' },
  };

  assert.deepEqual(fixedValueMappingErrors(fields, mappings), [
    { error: 'Class fixed value is required' },
  ]);
});
