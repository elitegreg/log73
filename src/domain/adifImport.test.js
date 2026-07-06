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

test('parseFirstAdifRecord reads typed tags', () => {
  const record = parseFirstAdifRecord('<CALL:4:S>W1AW<EOR>');

  assert.deepEqual(record.fields, {
    CALL: 'W1AW',
  });
});

test('parseFirstAdifRecord ignores typed header tags', () => {
  const record = parseFirstAdifRecord(
    'Log73\n<ADIF_VER:5:S>3.1.0<EOH>\n<CALL:4:S>W1AW<EOR>',
  );

  assert.deepEqual(record.fields, {
    CALL: 'W1AW',
  });
});

test('parseFirstAdifRecord ignores extra tag suffixes', () => {
  const record = parseFirstAdifRecord('<CALL:4:SOMETHING>W1AW<EOR>');

  assert.deepEqual(record.fields, {
    CALL: 'W1AW',
  });
});

test('parseFirstAdifRecord requires the length in the second tag segment', () => {
  const record = parseFirstAdifRecord('<CALL:S:4>W1AW<EOR>');

  assert.deepEqual(record.fields, {});
});

test('parseFirstAdifRecord accepts zero-length typed fields', () => {
  const record = parseFirstAdifRecord('<COMMENT:0:S><EOR>');

  assert.deepEqual(record.fields, {
    COMMENT: '',
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
