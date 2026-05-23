import React, { useCallback, useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';

const DEFAULT_RADIO_KIND = 'generic-elecraft';
const DEFAULT_TRANSPORT_KIND = 'tcp';

function CreateRadioScreen() {
  const navigate = useNavigate();
  const { radioId } = useParams();
  const { notifyError } = useNotifications();
  const isEditing = Boolean(radioId);
  const [radioKinds, setRadioKinds] = useState([DEFAULT_RADIO_KIND]);
  const [name, setName] = useState('');
  const [radioKind, setRadioKind] = useState(DEFAULT_RADIO_KIND);
  const [transportKind, setTransportKind] = useState(DEFAULT_TRANSPORT_KIND);
  const [tcpHost, setTcpHost] = useState('127.0.0.1');
  const [tcpPort, setTcpPort] = useState(5002);
  const [serialPort, setSerialPort] = useState('');
  const [serialBaudRate, setSerialBaudRate] = useState(115200);
  const [pollFrequency, setPollFrequency] = useState(0.25);
  const [catTimeout, setCatTimeout] = useState(2);
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
    let isCancelled = false;

    async function loadContext() {
      let kinds = [DEFAULT_RADIO_KIND];

      try {
        const result = await apiJson('/radio-kinds');
        if (Array.isArray(result) && result.length > 0) {
          kinds = result;
        }
      } catch (error) {
        notifyOperationalError(
          'CreateRadioScreen.loadRadioKinds',
          'Unable to load supported radio kinds.',
          error,
        );
      }

      if (isCancelled) return;
      setRadioKinds(kinds);

      if (!isEditing) {
        setRadioKind(kinds[0] || DEFAULT_RADIO_KIND);
        return;
      }

      const result = await apiJson(`/radios/${radioId}`);
      if (!result.ok) throw new Error(result.error ?? 'Radio not found');
      if (isCancelled) return;

      setName(result.radio.name ?? '');
      setRadioKind(result.radio.radio_kind ?? kinds[0] ?? DEFAULT_RADIO_KIND);
      setTransportKind(
        result.radio.transport_kind ?? DEFAULT_TRANSPORT_KIND,
      );
      setTcpHost(result.radio.tcp_host ?? '127.0.0.1');
      setTcpPort(result.radio.tcp_port ?? 5002);
      setSerialPort(result.radio.serial_port ?? '');
      setSerialBaudRate(result.radio.serial_baud_rate ?? 115200);
      setPollFrequency(result.radio.poll_frequency ?? 0.25);
      setCatTimeout(result.radio.cat_timeout ?? 2);
      setWinkeyerEnabled(Boolean(result.radio.winkeyer_enabled));
      setWinkeyerSerialPort(result.radio.winkeyer_serial_port ?? '');
    }

    loadContext().catch((error) =>
      notifyOperationalError(
        'CreateRadioScreen.loadContext',
        'Unable to load radio settings.',
        error,
        { radioId, isEditing },
      ),
    );

    return () => {
      isCancelled = true;
    };
  }, [isEditing, notifyOperationalError, radioId]);

  async function saveRadio(event) {
    event.preventDefault();
    const result = await apiJson(isEditing ? `/radios/${radioId}` : '/radios', {
      method: isEditing ? 'PUT' : 'POST',
      body: JSON.stringify({
        name,
        radio_kind: radioKind,
        transport_kind: transportKind,
        tcp_host: tcpHost,
        tcp_port: Number(tcpPort),
        serial_port: serialPort,
        serial_baud_rate: Number(serialBaudRate),
        poll_frequency: Number(pollFrequency),
        cat_timeout: Number(catTimeout),
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
        Radio Type
        <select
          value={radioKind}
          onChange={(event) => setRadioKind(event.target.value)}
          required
        >
          {[...new Set([radioKind, ...radioKinds])].map((kind) => (
            <option key={kind} value={kind}>
              {kind}
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
        Transport
        <select
          value={transportKind}
          onChange={(event) => setTransportKind(event.target.value)}
          required
        >
          <option value="tcp">TCP</option>
          <option value="serial">Serial</option>
        </select>
      </label>
      {transportKind === 'tcp' ? (
        <>
          <label>
            TCP Host
            <input
              value={tcpHost}
              onChange={(event) => setTcpHost(event.target.value)}
              required
            />
          </label>
          <label>
            TCP Port
            <input
              type="number"
              min="1"
              max="65535"
              value={tcpPort}
              onChange={(event) => setTcpPort(event.target.value)}
              required
            />
          </label>
        </>
      ) : (
        <>
          <label>
            Serial Port
            <input
              value={serialPort}
              onChange={(event) => setSerialPort(event.target.value)}
              required
              placeholder="/dev/ttyUSB0"
            />
          </label>
          <label>
            Serial Baud Rate
            <input
              type="number"
              min="1"
              value={serialBaudRate}
              onChange={(event) => setSerialBaudRate(event.target.value)}
              required
            />
          </label>
        </>
      )}
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
        CAT Timeout (seconds)
        <input
          type="number"
          min="0.01"
          step="0.01"
          value={catTimeout}
          onChange={(event) => setCatTimeout(event.target.value)}
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
