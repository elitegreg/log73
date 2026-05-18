import React, { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { apiJson } from '../lib/api';

function CreateRadioScreen() {
  const navigate = useNavigate();
  const [name, setName] = useState('');
  const [host, setHost] = useState('127.0.0.1');
  const [port, setPort] = useState(4532);
  const [pollFrequency, setPollFrequency] = useState(0.25);
  const [rigctldTimeout, setRigctldTimeout] = useState(2);
  const [winkeyerEnabled, setWinkeyerEnabled] = useState(false);
  const [winkeyerSerialPort, setWinkeyerSerialPort] = useState('');
  const [error, setError] = useState('');

  useEffect(() => {
    apiJson('/radios')
      .then((radios) => setPort(4532 + radios.length))
      .catch(() => setPort(4532));
  }, []);

  async function createRadio(event) {
    event.preventDefault();
    setError('');
    const result = await apiJson('/radios', {
      method: 'POST',
      body: JSON.stringify({
        name,
        rigctld_host: host,
        rigctld_port: Number(port),
        poll_frequency: Number(pollFrequency),
        rigctld_timeout: Number(rigctldTimeout),
        winkeyer_enabled: winkeyerEnabled,
        winkeyer_serial_port: winkeyerEnabled ? winkeyerSerialPort : '',
      }),
    });
    if (!result.ok) {
      setError(result.error ?? 'Unable to create radio');
      return;
    }
    navigate('/ui/open_log');
  }

  return (
    <form className="form-window" onSubmit={createRadio}>
      <div className="title-bar">Log73 - Create Radio</div>
      {error && <div className="error-message">{error}</div>}
      <label>Name
        <input value={name} onChange={(event) => setName(event.target.value)} required />
      </label>
      <label>rigctld Host
        <input value={host} onChange={(event) => setHost(event.target.value)} required />
      </label>
      <label>rigctld Port
        <input type="number" min="0" max="65535" value={port} onChange={(event) => setPort(event.target.value)} required />
      </label>
      <label>Poll Frequency (seconds)
        <input type="number" min="0.01" step="0.01" value={pollFrequency} onChange={(event) => setPollFrequency(event.target.value)} required />
      </label>
      <label>rigctld Timeout (seconds)
        <input type="number" min="0.01" step="0.01" value={rigctldTimeout} onChange={(event) => setRigctldTimeout(event.target.value)} required />
      </label>
      <label>
        <input type="checkbox" checked={winkeyerEnabled} onChange={(event) => setWinkeyerEnabled(event.target.checked)} />
        Enable Winkeyer
      </label>
      <label>Winkeyer Serial Port
        <input value={winkeyerSerialPort} onChange={(event) => setWinkeyerSerialPort(event.target.value)} required={winkeyerEnabled} disabled={!winkeyerEnabled} placeholder="/dev/ttyUSB0" />
      </label>
      <div className="selection-actions">
        <button className="cmd-btn primary" type="submit">Create</button>
        <button className="cmd-btn" type="button" onClick={() => navigate('/ui/open_log')}>Cancel</button>
      </div>
    </form>
  );
}

export default CreateRadioScreen;
