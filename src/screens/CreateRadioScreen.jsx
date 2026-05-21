import React, { useCallback, useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';

function CreateRadioScreen() {
  const navigate = useNavigate();
  const { radioId } = useParams();
  const { notifyError } = useNotifications();
  const isEditing = Boolean(radioId);
  const [name, setName] = useState('');
  const [host, setHost] = useState('127.0.0.1');
  const [port, setPort] = useState(4532);
  const [pollFrequency, setPollFrequency] = useState(0.25);
  const [rigctldTimeout, setRigctldTimeout] = useState(2);
  const [winkeyerEnabled, setWinkeyerEnabled] = useState(false);
  const [winkeyerSerialPort, setWinkeyerSerialPort] = useState('');

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

  useEffect(() => {
    if (isEditing) {
      apiJson(`/radios/${radioId}`)
        .then((result) => {
          if (!result.ok) throw new Error(result.error ?? 'Radio not found');
          setName(result.radio.name ?? '');
          setHost(result.radio.rigctld_host ?? '127.0.0.1');
          setPort(result.radio.rigctld_port ?? 4532);
          setPollFrequency(result.radio.poll_frequency ?? 0.25);
          setRigctldTimeout(result.radio.rigctld_timeout ?? 2);
          setWinkeyerEnabled(Boolean(result.radio.winkeyer_enabled));
          setWinkeyerSerialPort(result.radio.winkeyer_serial_port ?? '');
        })
        .catch((error) =>
          notifyOperationalError(
            'CreateRadioScreen.loadRadio',
            'Unable to load radio.',
            error,
            { radioId },
          ),
        );
      return;
    }

    apiJson('/radios')
      .then((radios) => setPort(4532 + radios.length))
      .catch(() => setPort(4532));
  }, [isEditing, notifyOperationalError, radioId]);

  async function saveRadio(event) {
    event.preventDefault();
    const result = await apiJson(isEditing ? `/radios/${radioId}` : '/radios', {
      method: isEditing ? 'PUT' : 'POST',
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
      notifyOperationalError(
        'CreateRadioScreen.saveRadio',
        `Unable to ${isEditing ? 'update' : 'create'} radio.`,
        result.error,
        {
          isEditing,
          radioId,
        },
      );
      return;
    }
    navigate('/ui/open_log');
  }

  return (
    <form className="form-window" onSubmit={saveRadio}>
      <div className="title-bar">
        Log73 - {isEditing ? 'Edit' : 'Create'} Radio
      </div>
      <label>
        Name
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          required
        />
      </label>
      <label>
        rigctld Host
        <input
          value={host}
          onChange={(event) => setHost(event.target.value)}
          required
        />
      </label>
      <label>
        rigctld Port
        <input
          type="number"
          min="0"
          max="65535"
          value={port}
          onChange={(event) => setPort(event.target.value)}
          required
        />
      </label>
      <label>
        Poll Frequency (seconds)
        <input
          type="number"
          min="0.01"
          step="0.01"
          value={pollFrequency}
          onChange={(event) => setPollFrequency(event.target.value)}
          required
        />
      </label>
      <label>
        rigctld Timeout (seconds)
        <input
          type="number"
          min="0.01"
          step="0.01"
          value={rigctldTimeout}
          onChange={(event) => setRigctldTimeout(event.target.value)}
          required
        />
      </label>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={winkeyerEnabled}
          onChange={(event) => setWinkeyerEnabled(event.target.checked)}
        />
        <span>Enable Winkeyer</span>
      </label>
      <label>
        Winkeyer Serial Port
        <input
          value={winkeyerSerialPort}
          onChange={(event) => setWinkeyerSerialPort(event.target.value)}
          required={winkeyerEnabled}
          disabled={!winkeyerEnabled}
          placeholder="/dev/ttyUSB0"
        />
      </label>
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

export default CreateRadioScreen;
