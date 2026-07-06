import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { sanitizeConfiguredValue } from '../domain/contactFields';
import {
  adifFieldOptionLabel,
  adifFieldOptions,
  fixedValueMappingErrors,
  parseFirstAdifRecord,
} from '../domain/adifImport';
import { apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';

function logLabel(log) {
  return log ? `${log.name} - ${log.station_callsign} - ${log.contest_id}` : '';
}

function fixedConfigValue(field, log) {
  const sourceParam = field?.source_param;
  const value = sourceParam ? log?.contest_params?.[sourceParam] : field?.default;
  if (value === undefined || value === null) return '';
  return sanitizeConfiguredValue(field, String(value));
}

function hasFixedConfigValue(field, log) {
  return field?.fixed === true && fixedConfigValue(field, log) !== '';
}

function mappingValue(mapping) {
  if (!mapping) return '';
  if (mapping.kind === 'fixed_config') return 'fixed_config';
  if (mapping.kind === 'fixed_value') return 'fixed_value';
  if (mapping.kind === 'adif_field') return `adif:${mapping.field}`;
  return '';
}

function defaultMapping(field, log, options) {
  if (options.some((option) => option.name === field.adif)) {
    return { kind: 'adif_field', field: field.adif, value: '' };
  }
  if (hasFixedConfigValue(field, log)) {
    return { kind: 'fixed_config', field: '', value: '' };
  }
  return {
    kind: 'fixed_value',
    field: '',
    value: sanitizeConfiguredValue(field, field.default ?? ''),
  };
}

function ImportAdifScreen() {
  const navigate = useNavigate();
  const { logId } = useParams();
  const numericLogId = Number(logId);
  const { notifyError } = useNotifications();
  const [log, setLog] = useState(null);
  const [settings, setSettings] = useState(null);
  const [filename, setFilename] = useState('');
  const [fileText, setFileText] = useState('');
  const [fieldOptions, setFieldOptions] = useState([]);
  const [mappings, setMappings] = useState({});
  const [importErrors, setImportErrors] = useState([]);
  const [importing, setImporting] = useState(false);

  const notifyOperationalError = useCallback(
    (source, fallback, error, details = {}) => {
      const message = errorMessage(error, fallback);
      notifyError(message, { dedupeKey: `${source}:${message}` });
      reportClientErrorLater({
        source,
        message,
        error,
        details,
      });
    },
    [notifyError],
  );

  const exchangeFields = useMemo(() => settings?.exchange ?? [], [settings]);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      const log = await apiJson(`/logs/${numericLogId}`);
      const contestSettings = await apiJson(
        `/contest-settings?contest_id=${encodeURIComponent(log.contest_id)}`,
      );
      if (cancelled) return;
      setLog(log);
      setSettings(contestSettings);
    }

    load().catch((error) =>
      notifyOperationalError(
        'ImportAdifScreen.load',
        'Unable to load import settings.',
        error,
        { logId: numericLogId },
      ),
    );

    return () => {
      cancelled = true;
    };
  }, [numericLogId, notifyOperationalError]);

  useEffect(() => {
    if (!fileText || !log || exchangeFields.length === 0) return;
    setMappings(
      Object.fromEntries(
        exchangeFields.map((field) => [
          field.adif,
          defaultMapping(field, log, fieldOptions),
        ]),
      ),
    );
  }, [exchangeFields, fieldOptions, fileText, log]);

  async function selectFile(event) {
    const file = event.target.files?.[0];
    setImportErrors([]);
    if (!file) {
      setFilename('');
      setFileText('');
      setFieldOptions([]);
      setMappings({});
      return;
    }

    try {
      const text = await file.text();
      const firstRecord = parseFirstAdifRecord(text);
      const options = adifFieldOptions(firstRecord.fields);
      setFilename(file.name);
      setFileText(text);
      setFieldOptions(options);
      setMappings(
        Object.fromEntries(
          exchangeFields.map((field) => [
            field.adif,
            defaultMapping(field, log, options),
          ]),
        ),
      );
    } catch (error) {
      notifyOperationalError(
        'ImportAdifScreen.selectFile',
        'Unable to read ADIF file.',
        error,
        { logId: numericLogId },
      );
    }
  }

  function updateMapping(field, value) {
    setMappings((current) => {
      const previous = current[field.adif] ?? defaultMapping(field, log, []);
      if (value === 'fixed_config') {
        return {
          ...current,
          [field.adif]: { ...previous, kind: 'fixed_config', field: '' },
        };
      }
      if (value === 'fixed_value') {
        return {
          ...current,
          [field.adif]: { ...previous, kind: 'fixed_value', field: '' },
        };
      }
      return {
        ...current,
        [field.adif]: {
          ...previous,
          kind: 'adif_field',
          field: value.replace(/^adif:/, ''),
        },
      };
    });
  }

  function updateFixedValue(field, value) {
    setMappings((current) => ({
      ...current,
      [field.adif]: {
        ...(current[field.adif] ?? {}),
        kind: 'fixed_value',
        field: '',
        value: sanitizeConfiguredValue(field, value),
      },
    }));
  }

  function importPayloadMappings() {
    return Object.fromEntries(
      exchangeFields.map((field) => {
        const mapping = mappings[field.adif] ?? defaultMapping(field, log, []);
        if (mapping.kind === 'adif_field') {
          return [field.adif, { kind: 'adif_field', field: mapping.field }];
        }
        if (mapping.kind === 'fixed_config') {
          return [field.adif, { kind: 'fixed_config' }];
        }
        return [
          field.adif,
          { kind: 'fixed_value', value: String(mapping.value ?? '') },
        ];
      }),
    );
  }

  async function importAdif(event) {
    event.preventDefault();
    setImportErrors([]);
    if (!fileText) {
      notifyError('Select an ADIF file before importing.', {
        dedupeKey: 'ImportAdifScreen.noFile',
      });
      return;
    }
    const mappingErrors = fixedValueMappingErrors(exchangeFields, mappings);
    if (mappingErrors.length > 0) {
      setImportErrors(mappingErrors);
      return;
    }

    setImporting(true);
    try {
      await apiJson(`/logs/${numericLogId}/adif/import`, {
        method: 'POST',
        body: JSON.stringify({
          adif: fileText,
          mappings: importPayloadMappings(),
        }),
      });
      navigate('/ui/open_log', { replace: true });
    } catch (error) {
      const payload = error?.payload;
      if (Array.isArray(payload?.errors) && payload.errors.length > 0) {
        setImportErrors(payload.errors);
      } else {
        notifyOperationalError(
          'ImportAdifScreen.import',
          'Unable to import ADIF file.',
          error,
          { logId: numericLogId },
        );
      }
    } finally {
      setImporting(false);
    }
  }

  return (
    <form className="form-window" onSubmit={importAdif}>
      <div className="title-bar">
        <span>Log73 - Import ADIF</span>
      </div>
      <label>
        Log
        <input value={logLabel(log)} readOnly />
      </label>
      <label>
        ADIF File
        <input
          type="file"
          accept=".adi,.adif"
          onChange={selectFile}
          disabled={!log || !settings}
        />
      </label>
      {filename ? <div className="form-note">Selected: {filename}</div> : null}
      <div className="form-note">
        QSO date/time, station callsign, call, band, frequency, and mode must be
        present in the ADIF file.
      </div>
      <div className="import-mapping-list">
        {exchangeFields.map((field) => {
          const mapping = mappings[field.adif] ?? defaultMapping(field, log, []);
          const selectValue = mappingValue(mapping);
          return (
            <div className="import-mapping-row" key={field.adif}>
              <label>
                {field.name} ({field.adif})
                <select
                  value={selectValue}
                  onChange={(event) => updateMapping(field, event.target.value)}
                  disabled={!fileText}
                >
                  {hasFixedConfigValue(field, log) ? (
                    <option value="fixed_config">
                      Fixed config ({fixedConfigValue(field, log)})
                    </option>
                  ) : null}
                  <option value="fixed_value">Fixed set</option>
                  {fieldOptions.map((option) => (
                    <option key={option.name} value={`adif:${option.name}`}>
                      {adifFieldOptionLabel(option)}
                    </option>
                  ))}
                </select>
              </label>
              {mapping.kind === 'fixed_value' ? (
                <label>
                  Value
                  <input
                    value={mapping.value ?? ''}
                    onChange={(event) =>
                      updateFixedValue(field, event.target.value)
                    }
                    disabled={!fileText}
                  />
                </label>
              ) : null}
            </div>
          );
        })}
      </div>
      {importErrors.length > 0 ? (
        <div className="error-message">
          {importErrors.map((error, index) => (
            <div key={`${error.line}:${index}`}>
              {Number.isFinite(error.line) ? `Line ${error.line}: ` : ''}
              {error.error}
            </div>
          ))}
        </div>
      ) : null}
      <div className="selection-actions">
        <button
          className="cmd-btn primary"
          type="submit"
          disabled={!fileText || importing}
        >
          {importing ? 'Importing...' : 'Import'}
        </button>
        <button
          className="cmd-btn"
          type="button"
          onClick={() => navigate('/ui/open_log')}
        >
          Cancel
        </button>
      </div>
    </form>
  );
}

export default ImportAdifScreen;
