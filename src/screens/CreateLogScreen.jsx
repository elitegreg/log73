import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import ConfiguredFields from '../components/ConfiguredFields';
import { sanitizeConfiguredValue } from '../domain/contactFields';
import { validateConfiguredField } from '../domain/validation';
import { apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';

function createFields(contest) {
  return [
    ...(contest?.log_params ?? []),
    ...(contest?.cabrillo?.log_fields ?? []),
  ];
}

function defaultParamValues(contest) {
  return Object.fromEntries(
    createFields(contest).map((param) => [param.name, param.default ?? '']),
  );
}

function normalizedParamValue(field, value) {
  const text = String(value ?? '').replace(/\r\n/g, '\n');
  return String(field.widget ?? '').toLowerCase() === 'textarea'
    ? text.trim()
    : text.trim();
}

function normalizedParamObject(fields, values) {
  return Object.fromEntries(
    fields.map((field) => [
      field.name,
      normalizedParamValue(field, values[field.name]),
    ]),
  );
}

function CreateLogScreen() {
  const navigate = useNavigate();
  const { logId } = useParams();
  const { notifyError } = useNotifications();
  const isEditing = Boolean(logId);
  const [name, setName] = useState('');
  const [stationCallsign, setStationCallsign] = useState('');
  const [contestId, setContestId] = useState('');
  const [contestSummaries, setContestSummaries] = useState([]);
  const [contestSettings, setContestSettings] = useState(null);
  const [contestParams, setContestParams] = useState({});

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

  const currentFields = useMemo(
    () => createFields(contestSettings),
    [contestSettings],
  );

  useEffect(() => {
    apiJson('/contest-rules')
      .then((rules) => {
        setContestSummaries(rules);
        if (!isEditing && rules.length > 0) {
          setContestId((currentContestId) =>
            rules.some((rule) => rule.contest === currentContestId)
              ? currentContestId
              : rules[0].contest,
          );
        }
      })
      .catch((error) =>
        notifyOperationalError(
          'CreateLogScreen.loadContestRules',
          'Unable to load contest rules.',
          error,
        ),
      );
  }, [isEditing, notifyOperationalError]);

  useEffect(() => {
    if (!contestId) {
      setContestSettings(null);
      return;
    }
    apiJson(`/contest-settings?contest_id=${encodeURIComponent(contestId)}`)
      .then((settings) => setContestSettings(settings))
      .catch((error) =>
        notifyOperationalError(
          'CreateLogScreen.loadContestSettings',
          'Unable to load contest settings.',
          error,
          { contestId },
        ),
      );
  }, [contestId, notifyOperationalError]);

  useEffect(() => {
    if (!isEditing) return;
    apiJson(`/logs/${logId}`)
      .then((result) => {
        if (!result.ok) throw new Error(result.error ?? 'Log not found');
        setName(result.log.name ?? '');
        setStationCallsign(result.log.station_callsign ?? '');
        setContestId(result.log.contest_id ?? '');
        setContestParams(result.log.contest_params ?? {});
      })
      .catch((error) =>
        notifyOperationalError(
          'CreateLogScreen.loadLog',
          'Unable to load log.',
          error,
          { logId },
        ),
      );
  }, [isEditing, logId, notifyOperationalError]);

  useEffect(() => {
    if (!contestSettings) return;
    const defaults = defaultParamValues(contestSettings);
    setContestParams((current) => ({ ...defaults, ...current }));
  }, [contestSettings]);

  function updateContestParam(param, value) {
    setContestParams((current) => ({
      ...current,
      [param.name]: sanitizeConfiguredValue(param, value),
    }));
  }

  async function saveLog(event) {
    event.preventDefault();
    const normalizedParams = normalizedParamObject(
      currentFields,
      contestParams,
    );
    const invalidField = currentFields.find((field) => {
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
        dedupeKey: `CreateLogScreen.invalid:${invalidField.name}`,
      });
      return;
    }

    const result = await apiJson(isEditing ? `/logs/${logId}` : '/logs', {
      method: isEditing ? 'PUT' : 'POST',
      body: JSON.stringify(
        isEditing
          ? {
              name,
              station_callsign: stationCallsign,
              contest_params: normalizedParams,
            }
          : {
              name,
              contest_id: contestId,
              station_callsign: stationCallsign,
              contest_params: normalizedParams,
            },
      ),
    });
    if (!result.ok) {
      notifyOperationalError(
        'CreateLogScreen.saveLog',
        `Unable to ${isEditing ? 'update' : 'create'} log.`,
        result.error,
        {
          isEditing,
          logId,
          contestId,
        },
      );
      return;
    }
    navigate('/ui/open_log');
  }

  return (
    <form className="form-window" onSubmit={saveLog}>
      <div className="title-bar">
        Log73 - {isEditing ? 'Edit' : 'Create'} Log
      </div>
      <label>
        Contest
        <select
          value={contestId}
          onChange={(event) => setContestId(event.target.value)}
          disabled={isEditing}
        >
          {contestSummaries.map((contest) => (
            <option key={contest.contest} value={contest.contest}>
              {contest.display_name || contest.contest}
            </option>
          ))}
        </select>
      </label>
      <label>
        Name
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          required
        />
      </label>
      <label>
        Station Callsign
        <input
          value={stationCallsign}
          onChange={(event) =>
            setStationCallsign(event.target.value.toUpperCase())
          }
          required
        />
      </label>
      <ConfiguredFields
        fields={currentFields}
        values={contestParams}
        onChange={updateContestParam}
      />
      <div className="selection-actions">
        <button className="cmd-btn primary" type="submit">
          {isEditing ? 'Save' : 'Create'}
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

export default CreateLogScreen;
