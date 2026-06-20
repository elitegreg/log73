import React, { useCallback, useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';

const DEFAULT_RADIO_KIND = 'dummy';
const DEFAULT_TRANSPORT_KIND = 'none';
const DEFAULT_REAL_RADIO_TRANSPORT_KIND = 'tcp';
const DEFAULT_CW_KEYER_TYPE = 'none';
const DEFAULT_CW_SERIAL_BAUD_RATE = 9600;
const DEFAULT_CW_SERIAL_LINE = 'dtr';
const DEFAULT_CW_TUNING_INCREMENT_HZ = 20;
const DEFAULT_SSB_TUNING_INCREMENT_HZ = 100;

function normalizeRadioKinds(value) {
  return (Array.isArray(value) ? value : [])
    .map((kind) =>
      typeof kind === 'string'
        ? { id: kind, display_name: kind, description: '' }
        : {
            id: String(kind?.id ?? '').trim(),
            display_name: String(kind?.display_name ?? kind?.id ?? '').trim(),
            description: String(kind?.description ?? '').trim(),
          },
    )
    .filter((kind) => kind.id);
}

function normalizeSerialPorts(value) {
  return (Array.isArray(value) ? value : [])
    .map((port) =>
      typeof port === 'string'
        ? { name: port, display_name: port }
        : {
            name: String(port?.name ?? '').trim(),
            display_name: String(
              port?.display_name ?? port?.description ?? port?.name ?? '',
            ).trim(),
          },
    )
    .filter((port) => port.name);
}

function defaultRadioKind(radioKinds) {
  return (
    radioKinds.find((kind) => kind.id === DEFAULT_RADIO_KIND)?.id ??
    radioKinds[0]?.id ??
    DEFAULT_RADIO_KIND
  );
}

function radioKindOptions(radioKinds, radioKind) {
  const options = new Map(radioKinds.map((kind) => [kind.id, kind]));
  if (radioKind && !options.has(radioKind)) {
    options.set(radioKind, {
      id: radioKind,
      display_name: radioKind,
      description: '',
    });
  }
  return [...options.values()].sort((left, right) => {
    if (left.id === DEFAULT_RADIO_KIND) return -1;
    if (right.id === DEFAULT_RADIO_KIND) return 1;
    return (left.display_name || left.id).localeCompare(
      right.display_name || right.id,
    );
  });
}

function radioKindLabel(kind) {
  return kind.display_name && kind.display_name !== kind.id
    ? `${kind.display_name} (${kind.id})`
    : kind.id;
}

function serialPortOptions(serialPorts, serialPort) {
  const options = new Map(serialPorts.map((port) => [port.name, port]));
  if (serialPort && !options.has(serialPort)) {
    options.set(serialPort, { name: serialPort, display_name: serialPort });
  }
  return [...options.values()];
}

function serialPortLabel(port) {
  return port.display_name || port.name;
}

function CreateRadioScreen() {
  const navigate = useNavigate();
  const { radioId } = useParams();
  const { notifyError } = useNotifications();
  const isEditing = Boolean(radioId);
  const [radioKinds, setRadioKinds] = useState([]);
  const [serialPorts, setSerialPorts] = useState([]);
  const [name, setName] = useState('');
  const [radioKind, setRadioKind] = useState('');
  const [transportKind, setTransportKind] = useState(DEFAULT_TRANSPORT_KIND);
  const [tcpHost, setTcpHost] = useState('127.0.0.1');
  const [tcpPort, setTcpPort] = useState('');
  const [serialPort, setSerialPort] = useState('');
  const [serialBaudRate, setSerialBaudRate] = useState(115200);
  const [options, setOptions] = useState('');
  const [cwTuningIncrementHz, setCwTuningIncrementHz] = useState(
    DEFAULT_CW_TUNING_INCREMENT_HZ,
  );
  const [ssbTuningIncrementHz, setSsbTuningIncrementHz] = useState(
    DEFAULT_SSB_TUNING_INCREMENT_HZ,
  );
  const [ritClearOnLog, setRitClearOnLog] = useState(false);
  const [cwKeyerType, setCwKeyerType] = useState(DEFAULT_CW_KEYER_TYPE);
  const [winkeyerSerialPort, setWinkeyerSerialPort] = useState('');
  const [cwSerialPort, setCwSerialPort] = useState('');
  const [cwSerialBaudRate, setCwSerialBaudRate] = useState(
    DEFAULT_CW_SERIAL_BAUD_RATE,
  );
  const [cwSerialLine, setCwSerialLine] = useState(DEFAULT_CW_SERIAL_LINE);
  const [defaultCwMessages, setDefaultCwMessages] = useState('');
  const [cwMessages, setCwMessages] = useState('');
  const [isCwMessagesOpen, setIsCwMessagesOpen] = useState(false);
  const [cwMessagesValidationMessage, setCwMessagesValidationMessage] =
    useState('');

  const selectedRadioKind = radioKind || defaultRadioKind(radioKinds);

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
    if (
      selectedRadioKind &&
      selectedRadioKind !== DEFAULT_RADIO_KIND &&
      transportKind === 'none'
    ) {
      setTransportKind(DEFAULT_REAL_RADIO_TRANSPORT_KIND);
    }
  }, [selectedRadioKind, transportKind]);

  useEffect(() => {
    let isCancelled = false;

    async function loadContext() {
      let kinds = [];

      try {
        const result = await apiJson('/radio-kinds');
        kinds = normalizeRadioKinds(result);
      } catch (error) {
        notifyOperationalError(
          'CreateRadioScreen.loadRadioKinds',
          'Unable to load supported radio kinds.',
          error,
        );
      }

      let serialPorts = [];
      try {
        const serialPortsResult = await apiJson('/serial-ports');
        if (serialPortsResult.ok) {
          serialPorts = normalizeSerialPorts(serialPortsResult.serial_ports);
        } else {
          throw new Error(
            serialPortsResult.error ?? 'Unable to load available serial ports.',
          );
        }
      } catch (error) {
        notifyOperationalError(
          'CreateRadioScreen.loadSerialPorts',
          'Unable to load available serial ports.',
          error,
        );
      }

      let loadedDefaultCwMessages = '';
      try {
        const defaultMessagesResult = await apiJson(
          '/radios/cw-messages/default',
        );
        if (defaultMessagesResult.ok) {
          loadedDefaultCwMessages = defaultMessagesResult.cw_messages ?? '';
        }
      } catch (error) {
        notifyOperationalError(
          'CreateRadioScreen.loadDefaultCwMessages',
          'Unable to load default CW messages.',
          error,
        );
      }

      if (isCancelled) return;
      setRadioKinds(kinds);
      setSerialPorts(serialPorts);
      setDefaultCwMessages(loadedDefaultCwMessages);

      if (!isEditing) {
        const nextRadioKind = defaultRadioKind(kinds);
        setRadioKind(nextRadioKind);
        setTransportKind(
          nextRadioKind === DEFAULT_RADIO_KIND
            ? DEFAULT_TRANSPORT_KIND
            : DEFAULT_REAL_RADIO_TRANSPORT_KIND,
        );
        setCwMessages(loadedDefaultCwMessages);
        return;
      }

      const result = await apiJson(`/radios/${radioId}`);
      if (!result.ok) throw new Error(result.error ?? 'Radio not found');
      if (isCancelled) return;

      setName(result.radio.name ?? '');
      setRadioKind(result.radio.radio_kind ?? '');
      setTransportKind(result.radio.transport_kind ?? DEFAULT_TRANSPORT_KIND);
      setTcpHost(result.radio.tcp_host ?? '127.0.0.1');
      setTcpPort(result.radio.tcp_port);
      setSerialPort(result.radio.serial_port ?? '');
      setSerialBaudRate(result.radio.serial_baud_rate ?? 115200);
      setOptions(result.radio.options ?? '');
      setCwTuningIncrementHz(
        result.radio.cw_tuning_increment_hz ?? DEFAULT_CW_TUNING_INCREMENT_HZ,
      );
      setSsbTuningIncrementHz(
        result.radio.ssb_tuning_increment_hz ?? DEFAULT_SSB_TUNING_INCREMENT_HZ,
      );
      setRitClearOnLog(Boolean(result.radio.rit_clear_on_log));
      setCwKeyerType(result.radio.cw_keyer_type ?? DEFAULT_CW_KEYER_TYPE);
      setWinkeyerSerialPort(result.radio.winkeyer_serial_port ?? '');
      setCwSerialPort(result.radio.cw_serial_port ?? '');
      setCwSerialBaudRate(
        result.radio.cw_serial_baud_rate ?? DEFAULT_CW_SERIAL_BAUD_RATE,
      );
      setCwSerialLine(result.radio.cw_serial_line ?? DEFAULT_CW_SERIAL_LINE);
      setCwMessages(result.radio.cw_messages ?? loadedDefaultCwMessages);
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

  async function validateCwMessages() {
    setCwMessagesValidationMessage('');
    const result = await apiJson('/radios/cw-messages/validate', {
      method: 'POST',
      body: JSON.stringify({ cw_messages: cwMessages }),
    });

    if (!result.ok) {
      setCwMessagesValidationMessage(
        result.error ?? 'CW messages are invalid.',
      );
      notifyOperationalError(
        'CreateRadioScreen.validateCwMessages',
        'CW messages are invalid.',
        result.error,
      );
      return false;
    }

    setCwMessagesValidationMessage('CW messages are valid.');
    return true;
  }

  function openHelpWindow() {
    window.open('/help/index.html', '_blank', 'noopener,noreferrer');
  }

  async function saveRadio(event) {
    event.preventDefault();
    if (!(await validateCwMessages())) return;

    const result = await apiJson(isEditing ? `/radios/${radioId}` : '/radios', {
      method: isEditing ? 'PUT' : 'POST',
      body: JSON.stringify({
        name,
        radio_kind: selectedRadioKind,
        transport_kind: transportKind,
        tcp_host: tcpHost,
        tcp_port: Number(tcpPort),
        serial_port: serialPort,
        serial_baud_rate: Number(serialBaudRate),
        options: options,
        cw_tuning_increment_hz: Number(cwTuningIncrementHz),
        ssb_tuning_increment_hz: Number(ssbTuningIncrementHz),
        rit_clear_on_log: Boolean(ritClearOnLog),
        cw_keyer_type: cwKeyerType,
        winkeyer_serial_port:
          cwKeyerType === 'winkeyer' ? winkeyerSerialPort : '',
        cw_serial_port: cwKeyerType === 'serial' ? cwSerialPort : '',
        cw_serial_baud_rate:
          cwKeyerType === 'serial'
            ? Number(cwSerialBaudRate)
            : DEFAULT_CW_SERIAL_BAUD_RATE,
        cw_serial_line:
          cwKeyerType === 'serial' ? cwSerialLine : DEFAULT_CW_SERIAL_LINE,
        cw_messages: cwMessages,
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
        <span>Log73 - {isEditing ? 'Edit' : 'Create'} Radio</span>
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
        Radio Type
        <select
          value={selectedRadioKind}
          onChange={(event) => setRadioKind(event.target.value)}
          required
        >
          {radioKindOptions(radioKinds, selectedRadioKind).map((kind) => (
            <option key={kind.id} value={kind.id}>
              {radioKindLabel(kind)}
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
          <option value="none">None / Dummy</option>
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
      ) : null}
      {transportKind === 'serial' ? (
        <>
          <label>
            Serial Port
            <select
              value={serialPort}
              onChange={(event) => setSerialPort(event.target.value)}
              required
            >
              <option value="" disabled>
                {serialPorts.length > 0
                  ? 'Select a serial port'
                  : 'No serial ports available'}
              </option>
              {serialPortOptions(serialPorts, serialPort).map((port) => (
                <option key={port.name} value={port.name}>
                  {serialPortLabel(port)}
                </option>
              ))}
            </select>
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
      ) : null}
      <label>
        Options
        <input
          value={options}
          onChange={(event) => setOptions(event.target.value)}
        />
      </label>
      <label>
        Tuning Increment (CW) in Hz
        <input
          type="number"
          min="1"
          value={cwTuningIncrementHz}
          onChange={(event) => setCwTuningIncrementHz(event.target.value)}
          required
        />
      </label>
      <label>
        Tuning Increment (SSB) in Hz
        <input
          type="number"
          min="1"
          value={ssbTuningIncrementHz}
          onChange={(event) => setSsbTuningIncrementHz(event.target.value)}
          required
        />
      </label>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={ritClearOnLog}
          onChange={(event) => setRitClearOnLog(event.target.checked)}
        />
        RIT Clear on Log
      </label>
      <label>
        CW Keying
        <select
          value={cwKeyerType}
          onChange={(event) => setCwKeyerType(event.target.value)}
          required
        >
          <option value="none">None</option>
          <option value="winkeyer">Winkeyer</option>
          <option value="cat">CAT</option>
          <option value="serial">Serial (DTR/RTS)</option>
        </select>
      </label>
      {cwKeyerType === 'winkeyer' ? (
        <label>
          Winkeyer Serial Port
          <input
            value={winkeyerSerialPort}
            onChange={(event) => setWinkeyerSerialPort(event.target.value)}
            required
            placeholder="/dev/ttyUSB0"
          />
        </label>
      ) : null}
      {cwKeyerType === 'serial' ? (
        <>
          <label>
            CW Serial Port
            <select
              value={cwSerialPort}
              onChange={(event) => setCwSerialPort(event.target.value)}
              required
            >
              <option value="" disabled>
                {serialPorts.length > 0
                  ? 'Select a serial port'
                  : 'No serial ports available'}
              </option>
              {serialPortOptions(serialPorts, cwSerialPort).map((port) => (
                <option key={port.name} value={port.name}>
                  {serialPortLabel(port)}
                </option>
              ))}
            </select>
          </label>
          <label>
            CW Serial Baud Rate
            <input
              type="number"
              min="1"
              value={cwSerialBaudRate}
              onChange={(event) => setCwSerialBaudRate(event.target.value)}
              required
            />
          </label>
          <label>
            DTR/RTS Selection
            <select
              value={cwSerialLine}
              onChange={(event) => setCwSerialLine(event.target.value)}
              required
            >
              <option value="dtr">DTR</option>
              <option value="rts">RTS</option>
            </select>
          </label>
        </>
      ) : null}
      <div className="selection-actions">
        <button
          className="cmd-btn"
          type="button"
          onClick={() => setIsCwMessagesOpen((current) => !current)}
        >
          {isCwMessagesOpen ? 'Hide CW Messages' : 'Edit CW Messages'}
        </button>
      </div>
      {isCwMessagesOpen ? (
        <div className="cw-messages-editor">
          <label>
            CW Messages
            <textarea
              value={cwMessages}
              onChange={(event) => {
                setCwMessages(event.target.value);
                setCwMessagesValidationMessage('');
              }}
              rows={18}
              spellCheck={false}
            />
          </label>
          {cwMessagesValidationMessage ? (
            <div className="cw-messages-validation-status">
              {cwMessagesValidationMessage}
            </div>
          ) : null}
          <div className="selection-actions">
            <button
              className="cmd-btn"
              type="button"
              onClick={() => setCwMessages(defaultCwMessages)}
              disabled={!defaultCwMessages}
            >
              Reset to Defaults
            </button>
            <button
              className="cmd-btn"
              type="button"
              onClick={() => validateCwMessages()}
            >
              Validate CW Messages
            </button>
          </div>
        </div>
      ) : null}
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
