import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import ConfiguredFields from '../components/ConfiguredFields';
import { sanitizeConfiguredValue } from '../domain/contactFields';
import {
  defaultExportValues,
  loadStoredExportValues,
  saveStoredExportValues,
} from '../domain/exportSettings';
import { validateConfiguredField } from '../domain/validation';
import { apiDownload, apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';

function normalizedExportValues(fields, values) {
  return Object.fromEntries(
    fields.map((field) => [
      field.name,
      String(values[field.name] ?? '')
        .replace(/\r\n/g, '\n')
        .trim(),
    ]),
  );
}

function ExportLogScreen() {
  const navigate = useNavigate();
  const { logId } = useParams();
  const numericLogId = Number(logId);
  const { notifyError } = useNotifications();
  const [log, setLog] = useState(null);
  const [settings, setSettings] = useState(null);
  const [exportParams, setExportParams] = useState({});

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

  const exportFields = useMemo(
    () => settings?.cabrillo?.export_fields ?? [],
    [settings],
  );

  useEffect(() => {
    let cancelled = false;

    async function load() {
      const logResult = await apiJson(`/logs/${numericLogId}`);
      if (!logResult.ok) throw new Error(logResult.error ?? 'Log not found');
      const contestSettings = await apiJson(
        `/contest-settings?contest_id=${encodeURIComponent(logResult.log.contest_id)}`,
      );
      if (cancelled) return;
      setLog(logResult.log);
      setSettings(contestSettings);
      setExportParams(
        defaultExportValues(
          contestSettings,
          logResult.log,
          loadStoredExportValues(logResult.log.contest_id),
        ),
      );
    }

    load().catch((error) =>
      notifyOperationalError(
        'ExportLogScreen.load',
        'Unable to load export settings.',
        error,
        { logId: numericLogId },
      ),
    );

    return () => {
      cancelled = true;
    };
  }, [numericLogId, notifyOperationalError]);

  function updateExportParam(field, value) {
    setExportParams((current) => ({
      ...current,
      [field.name]: sanitizeConfiguredValue(field, value),
    }));
  }

  function openHelpWindow() {
    window.open('/help/index.html', '_blank', 'noopener,noreferrer');
  }

  async function exportLog(event) {
    event.preventDefault();
    const normalizedParams = normalizedExportValues(exportFields, exportParams);
    const invalidField = exportFields.find((field) => {
      const validation = validateConfiguredField(
        field,
        normalizedParams[field.name] ?? '',
      );
      return !validation.ok;
    });
    if (invalidField) {
      const validation = validateConfiguredField(
        invalidField,
        normalizedParams[invalidField.name] ?? '',
      );
      notifyError(validation.error, {
        dedupeKey: `ExportLogScreen.invalid:${invalidField.name}`,
      });
      return;
    }

    try {
      const { blob, filename } = await apiDownload(
        `/logs/${numericLogId}/cabrillo`,
        {
          method: 'POST',
          body: JSON.stringify({ export_params: normalizedParams }),
        },
      );
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = filename;
      document.body.appendChild(link);
      link.click();
      link.remove();
      if (log?.contest_id) {
        saveStoredExportValues(log.contest_id, normalizedParams);
      }
      window.setTimeout(() => {
        URL.revokeObjectURL(url);
        navigate('/ui/open_log', { replace: true });
      }, 0);
    } catch (error) {
      notifyOperationalError(
        'ExportLogScreen.export',
        'Unable to export Cabrillo file.',
        error,
        { logId: numericLogId },
      );
    }
  }

  return (
    <form className="form-window" onSubmit={exportLog}>
      <div className="title-bar">
        <span>Log73 - Export Cabrillo</span>
        <button
          className="title-button title-help-button"
          type="button"
          aria-label="Open help"
          title="Open help"
          onClick={openHelpWindow}
        >
          ?
        </button>
      </div>
      <label>
        Log
        <input
          value={
            log
              ? `${log.name} - ${log.station_callsign} - ${log.contest_id}`
              : ''
          }
          readOnly
        />
      </label>
      <div className="form-note">
        Generated automatically: CREATED-BY, CALLSIGN, CONTEST, CLAIMED-SCORE,
        OPERATORS.
      </div>
      <ConfiguredFields
        fields={exportFields}
        values={exportParams}
        onChange={updateExportParam}
      />
      <div className="selection-actions">
        <button className="cmd-btn primary" type="submit">
          Export Cabrillo
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

export default ExportLogScreen;
