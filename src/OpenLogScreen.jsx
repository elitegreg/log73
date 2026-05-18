import React, { useEffect, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { apiJson } from './api';
import { THEME_OPTIONS } from './themes';

function OpenLogScreen({ theme, onSetTheme }) {
  const navigate = useNavigate();
  const [logs, setLogs] = useState([]);
  const [radios, setRadios] = useState([]);
  const [selectedLogId, setSelectedLogId] = useState('');
  const [selectedRadioId, setSelectedRadioId] = useState('');
  const [error, setError] = useState('');

  async function load() {
    const [nextLogs, nextRadios] = await Promise.all([
      apiJson('/logs'),
      apiJson('/radios'),
    ]);
    setLogs(nextLogs);
    setRadios(nextRadios);
    setSelectedLogId((current) => current || String(nextLogs[0]?.id ?? ''));
    setSelectedRadioId((current) => current || String(nextRadios[0]?.id ?? ''));
  }

  useEffect(() => {
    load().catch((err) => setError(err.message));
  }, []);

  async function deleteLog() {
    if (!selectedLogId) return;
    const result = await apiJson(`/logs/${selectedLogId}`, { method: 'DELETE' });
    if (!result.ok) {
      setError(result.error ?? 'Unable to delete log');
      return;
    }
    setSelectedLogId('');
    await load();
  }

  async function deleteRadio() {
    if (!selectedRadioId) return;
    const result = await apiJson(`/radios/${selectedRadioId}`, { method: 'DELETE' });
    if (!result.ok) {
      setError(result.error ?? 'Unable to delete radio');
      return;
    }
    setSelectedRadioId('');
    await load();
  }

  function openLogger() {
    setError('');
    if (!selectedLogId || !selectedRadioId) {
      setError('Select both a log and a radio before opening the logger.');
      return;
    }
    navigate(`/ui/logger/${selectedLogId}/${selectedRadioId}`);
  }

  return (
    <div className="selection-window">
      <div className="title-bar">Log73 - Open Log</div>
      <label className="theme-selector">
        Theme:
        <select value={theme} onChange={(event) => onSetTheme?.(event.target.value)}>
          {THEME_OPTIONS.map((themeOption) => (
            <option key={themeOption.id} value={themeOption.id}>{themeOption.label}</option>
          ))}
        </select>
      </label>
      {error && <div className="error-message">{error}</div>}
      <div className="selection-grid">
        <section>
          <h2>Logs</h2>
          <select className="selection-list" size={10} value={selectedLogId} onChange={(event) => setSelectedLogId(event.target.value)}>
            {logs.map((log) => (
              <option key={log.id} value={log.id}>{log.name} - {log.station_callsign} - {log.contest_id}</option>
            ))}
          </select>
          <div className="selection-buttons">
            <Link className="cmd-btn" to="/ui/create_log">Create</Link>
            <button className="cmd-btn" onClick={deleteLog}>Delete</button>
          </div>
        </section>
        <section>
          <h2>Radios</h2>
          <select className="selection-list" size={10} value={selectedRadioId} onChange={(event) => setSelectedRadioId(event.target.value)}>
            {radios.map((radio) => (
              <option key={radio.id} value={radio.id}>
                {radio.name} - {radio.rigctld_host}:{radio.rigctld_port} - poll {radio.poll_frequency}s timeout {radio.rigctld_timeout}s
              </option>
            ))}
          </select>
          <div className="selection-buttons">
            <Link className="cmd-btn" to="/ui/create_radio">Create</Link>
            <button className="cmd-btn" onClick={deleteRadio}>Delete</button>
          </div>
        </section>
      </div>
      <div className="selection-actions">
        <button className="cmd-btn primary" onClick={openLogger}>Open</button>
      </div>
    </div>
  );
}

export default OpenLogScreen;
