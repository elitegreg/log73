import assert from 'node:assert/strict';
import test from 'node:test';
import { defaultExportValues, exportSettingsStorageKey } from './exportSettings.js';

test('defaultExportValues prefers stored values over log params and YAML defaults', () => {
  const settings = {
    cabrillo: {
      export_fields: [
        { name: 'NAME', default: 'Rule Default' },
        { name: 'EMAIL', default: 'rule@example.com' },
      ],
    },
  };
  const log = {
    contest_params: {
      NAME: 'Contest Param Name',
      EMAIL: 'contest@example.com',
    },
  };
  const storedValues = {
    NAME: 'Stored Name',
  };

  assert.deepEqual(defaultExportValues(settings, log, storedValues), {
    NAME: 'Stored Name',
    EMAIL: 'contest@example.com',
  });
});

test('defaultExportValues falls back LOCATION to existing contest params', () => {
  const settings = {
    cabrillo: {
      export_fields: [{ name: 'LOCATION' }],
    },
  };
  const log = {
    contest_params: {
      County: 'ABBE',
    },
  };

  assert.deepEqual(defaultExportValues(settings, log), {
    LOCATION: 'ABBE',
  });
});

test('defaultExportValues preserves blank stored values', () => {
  const settings = {
    cabrillo: {
      export_fields: [{ name: 'LOCATION', default: 'SC' }],
    },
  };
  const log = {
    contest_params: {
      County: 'ABBE',
    },
  };

  assert.deepEqual(defaultExportValues(settings, log, { LOCATION: '' }), {
    LOCATION: '',
  });
});

test('exportSettingsStorageKey namespaces settings by contest id', () => {
  assert.equal(
    exportSettingsStorageKey('SC-QSO-PARTY (In State)'),
    'cabrilloExportSettings:SC-QSO-PARTY (In State)',
  );
});
