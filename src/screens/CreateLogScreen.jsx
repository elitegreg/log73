import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { apiJson } from '../lib/api';

function CreateLogScreen() {
  const navigate = useNavigate();
  const [name, setName] = useState('');
  const [stationCallsign, setStationCallsign] = useState('');
  const [contestId, setContestId] = useState('SC-QSO-PARTY');
  const [error, setError] = useState('');

  async function createLog(event) {
    event.preventDefault();
    setError('');
    const result = await apiJson('/logs', {
      method: 'POST',
      body: JSON.stringify({ name, contest_id: contestId, station_callsign: stationCallsign }),
    });
    if (!result.ok) {
      setError(result.error ?? 'Unable to create log');
      return;
    }
    navigate('/ui/open_log');
  }

  return (
    <form className="form-window" onSubmit={createLog}>
      <div className="title-bar">Log73 - Create Log</div>
      {error && <div className="error-message">{error}</div>}
      <label>Contest
        <select value={contestId} onChange={(event) => setContestId(event.target.value)}>
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
        <button className="cmd-btn primary" type="submit">Create</button>
        <button className="cmd-btn" type="button" onClick={() => navigate('/ui/open_log')}>Cancel</button>
      </div>
    </form>
  );
}

export default CreateLogScreen;
