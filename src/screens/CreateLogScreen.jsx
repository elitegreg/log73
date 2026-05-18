import React, { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { apiJson } from '../lib/api';

function CreateLogScreen() {
  const navigate = useNavigate();
  const { logId } = useParams();
  const isEditing = Boolean(logId);
  const [name, setName] = useState('');
  const [stationCallsign, setStationCallsign] = useState('');
  const [contestId, setContestId] = useState('SC-QSO-PARTY');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!isEditing) return;
    apiJson(`/logs/${logId}`)
      .then((result) => {
        if (!result.ok) throw new Error(result.error ?? 'Log not found');
        setName(result.log.name ?? '');
        setStationCallsign(result.log.station_callsign ?? '');
        setContestId(result.log.contest_id ?? 'SC-QSO-PARTY');
      })
      .catch((err) => setError(err.message));
  }, [isEditing, logId]);

  async function saveLog(event) {
    event.preventDefault();
    setError('');
    const result = await apiJson(isEditing ? `/logs/${logId}` : '/logs', {
      method: isEditing ? 'PUT' : 'POST',
      body: JSON.stringify(
        isEditing
          ? { name, station_callsign: stationCallsign }
          : { name, contest_id: contestId, station_callsign: stationCallsign },
      ),
    });
    if (!result.ok) {
      setError(result.error ?? `Unable to ${isEditing ? 'update' : 'create'} log`);
      return;
    }
    navigate('/ui/open_log');
  }

  return (
    <form className="form-window" onSubmit={saveLog}>
      <div className="title-bar">Log73 - {isEditing ? 'Edit' : 'Create'} Log</div>
      {error && <div className="error-message">{error}</div>}
      <label>Contest
        <select value={contestId} onChange={(event) => setContestId(event.target.value)} disabled={isEditing}>
          <option value="SC-QSO-PARTY">SC-QSO-PARTY</option>
        </select>
      </label>
      <label>Name
        <input value={name} onChange={(event) => setName(event.target.value)} required />
      </label>
      <label>Station Callsign
        <input value={stationCallsign} onChange={(event) => setStationCallsign(event.target.value.toUpperCase())} required />
      </label>
      <div className="selection-actions">
        <button className="cmd-btn primary" type="submit">{isEditing ? 'Save' : 'Create'}</button>
        <button className="cmd-btn" type="button" onClick={() => navigate('/ui/open_log')}>Cancel</button>
      </div>
    </form>
  );
}

export default CreateLogScreen;
