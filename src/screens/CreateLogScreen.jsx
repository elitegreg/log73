import React, { useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { parseFieldType, sanitizeExchangeValue } from '../domain/contactFields';
import { apiJson } from '../lib/api';

function paramsObject(params) {
  return Object.fromEntries(
    Object.entries(params).map(([key, value]) => [
      key,
      String(value).trim().toUpperCase(),
    ]),
  );
}

function defaultParamValues(contest) {
  return Object.fromEntries(
    (contest?.log_params ?? []).map((param) => [
      param.name,
      param.default ?? '',
    ]),
  );
}

function CreateLogScreen() {
  const navigate = useNavigate();
  const { logId } = useParams();
  const isEditing = Boolean(logId);
  const [name, setName] = useState('');
  const [stationCallsign, setStationCallsign] = useState('');
  const [contestId, setContestId] = useState('');
  const [contestRules, setContestRules] = useState([]);
  const [contestParams, setContestParams] = useState({});
  const [error, setError] = useState('');

  const selectedContest = useMemo(
    () => contestRules.find((contest) => contest.contest === contestId),
    [contestRules, contestId],
  );

  useEffect(() => {
    apiJson('/contest-rules')
      .then((rules) => {
        setContestRules(rules);
        if (!isEditing && rules.length > 0) {
          setContestId((currentContestId) =>
            rules.some((rule) => rule.contest === currentContestId)
              ? currentContestId
              : rules[0].contest,
          );
        }
      })
      .catch((err) => setError(err.message));
  }, [isEditing]);

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
      .catch((err) => setError(err.message));
  }, [isEditing, logId]);

  useEffect(() => {
    if (isEditing || !selectedContest) return;
    setContestParams(defaultParamValues(selectedContest));
  }, [isEditing, selectedContest]);

  function updateContestParam(param, value) {
    setContestParams((current) => ({
      ...current,
      [param.name]: sanitizeExchangeValue({ type: param.type }, value),
    }));
  }

  async function saveLog(event) {
    event.preventDefault();
    setError('');
    const normalizedParams = paramsObject(contestParams);
    if (!isEditing) {
      const missingParam = (selectedContest?.log_params ?? []).find(
        (param) =>
          param.required !== false &&
          String(normalizedParams[param.name] ?? '').trim() === '',
      );
      if (missingParam) {
        setError(`${missingParam.label ?? missingParam.name} is required.`);
        return;
      }
    }

    const result = await apiJson(isEditing ? `/logs/${logId}` : '/logs', {
      method: isEditing ? 'PUT' : 'POST',
      body: JSON.stringify(
        isEditing
          ? { name, station_callsign: stationCallsign }
          : {
              name,
              contest_id: contestId,
              station_callsign: stationCallsign,
              contest_params: normalizedParams,
            },
      ),
    });
    if (!result.ok) {
      setError(
        result.error ?? `Unable to ${isEditing ? 'update' : 'create'} log`,
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
      {error && <div className="error-message">{error}</div>}
      <label>
        Contest
        <select
          value={contestId}
          onChange={(event) => setContestId(event.target.value)}
          disabled={isEditing}
        >
          {contestRules.map((contest) => (
            <option key={contest.contest} value={contest.contest}>
              {contest.contest}
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
      {(selectedContest?.log_params ?? []).map((param) => {
        const { kind, maxLength } = parseFieldType(param.type);
        return (
          <label key={param.name}>
            {param.label ?? param.name}
            <input
              value={contestParams[param.name] ?? ''}
              onChange={(event) =>
                updateContestParam(param, event.target.value)
              }
              required={param.required !== false}
              pattern={param.regex ?? undefined}
              inputMode={kind === 'NUMERIC' ? 'numeric' : 'text'}
              maxLength={maxLength}
              disabled={isEditing}
            />
          </label>
        );
      })}
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
