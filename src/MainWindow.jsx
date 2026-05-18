import React, { useEffect, useRef, useState } from 'react';
import { fieldDefault, parseFieldType, sanitizeCallsign, sanitizeExchangeValue } from './contactFields';
import './App.css';

const MODE_OPTIONS = ['CW', 'SSB', 'FM', 'AM'];
const CALLSIGN_FIELD_WIDTH_CHARS = 13;
const AMATEUR_BANDS = [
  { meters: 160, name: '160m', lowerHz: 1800000, upperHz: 2000000 },
  { meters: 80, name: '80m', lowerHz: 3500000, upperHz: 4000000 },
  { meters: 60, name: '60m', lowerHz: 5330500, upperHz: 5406500 },
  { meters: 40, name: '40m', lowerHz: 7000000, upperHz: 7300000 },
  { meters: 30, name: '30m', lowerHz: 10100000, upperHz: 10150000 },
  { meters: 20, name: '20m', lowerHz: 14000000, upperHz: 14350000 },
  { meters: 17, name: '17m', lowerHz: 18068000, upperHz: 18168000 },
  { meters: 15, name: '15m', lowerHz: 21000000, upperHz: 21450000 },
  { meters: 12, name: '12m', lowerHz: 24890000, upperHz: 24990000 },
  { meters: 10, name: '10m', lowerHz: 28000000, upperHz: 29700000 },
  { meters: 6, name: '6m', lowerHz: 50000000, upperHz: 54000000 },
  { meters: 2, name: '2m', lowerHz: 144000000, upperHz: 148000000 },
];

function exchangeDefaults(settings, radioMode) {
  return Object.fromEntries(
    (settings?.exchange ?? []).map((field) => [field.name, fieldDefault(field, radioMode)]),
  );
}

function formatFrequency(frequencyHz) {
  return Math.round(frequencyHz / 1000);
}

function isFrequencyInput(value) {
  return /^\d+(\.\d+)?$/.test(value.trim());
}

function bandForFrequency(frequencyHz) {
  return AMATEUR_BANDS.find(
    (band) => frequencyHz >= band.lowerHz && frequencyHz <= band.upperHz,
  );
}

function bandByMeters(meters) {
  return AMATEUR_BANDS.find((band) => band.meters === meters);
}

function createContactId(date, callSign) {
  if (window.crypto?.randomUUID) {
    return window.crypto.randomUUID();
  }

  return `${date.getTime()}-${callSign}-${Math.random().toString(36).slice(2)}`;
}

function MainWindow({
  settings,
  log,
  radio,
  stationCallsign,
  operatorCallsign,
  radioState,
  backendSocketStatus,
  sessionId,
  logId,
  onSetRadioFrequency,
  onSetRadioMode,
  onLogContact,
  onExit,
}) {
  const [callSign, setCallSign] = useState('');
  const [exchangeValues, setExchangeValues] = useState({});
  const [operatingMode, setOperatingMode] = useState('S&P');
  const [cwWpm, setCwWpm] = useState(20);
  const callSignRef = useRef(null);
  const exchangeInputRefs = useRef({});
  const callSignEditedAtRef = useRef(new Date());
  const radioMode = radioState?.mode ?? 'CW';
  const radioFrequencyHz = radioState?.frequency_hz ?? 14025000;
  const allowedBands = settings?.allowed_bands ?? [];
  const currentBand = bandForFrequency(radioFrequencyHz);
  const currentBandValue = currentBand ? String(currentBand.meters) : 'unknown';
  const currentBandAllowed = currentBand
    ? allowedBands.includes(currentBand.meters)
    : false;
  const bandOptions = allowedBands
    .map(bandByMeters)
    .filter(Boolean);

  if (currentBand && !bandOptions.some((band) => band.meters === currentBand.meters)) {
    bandOptions.push(currentBand);
  }

  useEffect(() => {
    setExchangeValues(exchangeDefaults(settings, radioMode));
  }, [settings, radioMode]);

  function updateExchangeField(field, value) {
    setExchangeValues((current) => ({
      ...current,
      [field.name]: sanitizeExchangeValue(field, value, radioMode),
    }));
  }

  function handleCallsignChange(event) {
    setCallSign(sanitizeCallsign(event.target.value));
    callSignEditedAtRef.current = new Date();
  }

  function allRequiredFieldsFilled() {
    return (
      Boolean(settings?.exchange) &&
      callSign.trim() !== '' &&
      settings.exchange.every((field) => String(exchangeValues[field.name] ?? '').trim() !== '')
    );
  }

  function resetEntryFields() {
    setCallSign('');
    setExchangeValues(exchangeDefaults(settings, radioMode));
    callSignEditedAtRef.current = new Date();
    callSignRef.current?.focus();
  }

  function logContact() {
    if (!allRequiredFieldsFilled()) {
      return false;
    }

    const timeOn = callSignEditedAtRef.current;
    const normalizedCallSign = callSign.trim().toUpperCase();
    const contact = {
      QSO_DATE_TIME_ON: Math.floor(timeOn.getTime() / 1000),
      STATION_CALLSIGN: stationCallsign,
      OPERATOR: operatorCallsign,
      CONTEST_ID: settings.contest,
      CALL: normalizedCallSign,
      BAND: currentBand?.name ?? '',
      FREQ: radioFrequencyHz,
      MODE: radioMode,
      _status: 'Pending',
      _session_id: sessionId,
      _log_id: logId,
      _client_id: createContactId(timeOn, normalizedCallSign),
    };

    for (const field of settings.exchange) {
      contact[field.adif] = String(exchangeValues[field.name] ?? '').trim().toUpperCase();
    }

    onLogContact?.(contact);
    resetEntryFields();
    return true;
  }

  function focusNextEmptyField(currentFieldName) {
    const fields = [
      { name: 'CALL', value: callSign, ref: callSignRef, editable: true },
      ...(settings?.exchange ?? []).map((field) => ({
        name: field.name,
        value: exchangeValues[field.name] ?? '',
        ref: { current: exchangeInputRefs.current[field.name] },
        editable: field.fixed !== true,
      })),
    ];
    const currentIndex = fields.findIndex((field) => field.name === currentFieldName);
    const nextEmptyField = fields
      .slice(currentIndex + 1)
      .find((field) => field.editable && String(field.value).trim() === '');

    if (!nextEmptyField) {
      return false;
    }

    nextEmptyField.ref.current?.focus();
    return true;
  }

  function handleFieldTab(event, currentFieldName) {
    if (event.key !== 'Tab' || event.shiftKey) {
      return;
    }

    if (focusNextEmptyField(currentFieldName)) {
      event.preventDefault();
    }
  }

  function handleCallsignKeyDown(event) {
    const value = callSign.trim();

    if (event.key === 'Tab') {
      handleFieldTab(event, 'CALL');
      return;
    }

    if (event.key === 'Enter' && isFrequencyInput(value)) {
      event.preventDefault();
      onSetRadioFrequency?.(Math.round(Number.parseFloat(value) * 1000));
      setCallSign('');
      return;
    }

    if (event.key === 'Enter' && allRequiredFieldsFilled()) {
      event.preventDefault();
      logContact();
    }
  }

  function handleBandChange(event) {
    const selectedBand = bandByMeters(Number.parseInt(event.target.value, 10));

    if (selectedBand) {
      onSetRadioFrequency?.(selectedBand.lowerHz);
      onSetRadioMode?.(radioMode);
    }
  }

  function handleExchangeKeyDown(event, index) {
    if (event.key === 'Enter' && allRequiredFieldsFilled()) {
      event.preventDefault();
      logContact();
      return;
    }

    handleFieldTab(event, settings.exchange[index]?.name);
  }

  function handleCwWpmChange(event) {
    const wpm = Number.parseInt(event.target.value, 10);
    setCwWpm(Number.isFinite(wpm) ? wpm : 20);
  }

  return (
    <div className="window">
      <div className="title-bar logger-title-bar">
        <span>
          Log73 | Log: {log?.name ?? 'Loading...'} | Radio: {radio?.name ?? 'Loading...'} | Contest: {settings?.contest ?? 'Loading...'} | Mode: {radioMode}, Freq: {formatFrequency(radioFrequencyHz)}
        </span>
        <button className="title-button" onClick={onExit}>Exit Logger</button>
      </div>
      <div className="radio-controls">
        <label className="radio-control">
          Run Mode:
          <select value={operatingMode} onChange={(event) => setOperatingMode(event.target.value)}>
            <option value="S&P">S&amp;P</option>
            <option value="Run">Run</option>
          </select>
        </label>
        <label className={currentBandAllowed ? 'radio-control' : 'radio-control unsupported'}>
          Band:
          <select value={currentBandValue} onChange={handleBandChange}>
            {bandOptions.map((band) => (
              <option key={band.meters} value={band.meters}>
                {band.name}
              </option>
            ))}
            {!currentBand && <option value="unknown">Unknown</option>}
          </select>
        </label>
        <label className="radio-control">
          Mode:
          <select value={radioMode} onChange={(event) => onSetRadioMode?.(event.target.value)}>
            {MODE_OPTIONS.map((mode) => (
              <option key={mode} value={mode}>
                {mode}
              </option>
            ))}
          </select>
        </label>
        {radioMode === 'CW' && (
          <label className="radio-control cw-wpm-control">
            CW WPM:
            <input
              type="number"
              min="5"
              max="60"
              step="1"
              value={cwWpm}
              onChange={handleCwWpmChange}
            />
          </label>
        )}
        <div className="backend-socket-status" title={`Server ${backendSocketStatus}`}>
          <span
            className={`backend-socket-light ${backendSocketStatus === 'connected' ? 'connected' : 'disconnected'}`}
            aria-hidden="true"
          />
          Server
        </div>
      </div>
      <div className="entry-fields">
        <label
          className="entry-field"
          style={{ flex: `${CALLSIGN_FIELD_WIDTH_CHARS} 1 ${CALLSIGN_FIELD_WIDTH_CHARS}em` }}
        >
          <span>Callsign</span>
          <input
            ref={callSignRef}
            type="text"
            value={callSign}
            onChange={handleCallsignChange}
            onKeyDown={handleCallsignKeyDown}
            className="callsign"
            maxLength={12}
          />
        </label>
        {settings?.exchange?.map((field, index) => {
          const { kind, maxLength } = parseFieldType(field.type, radioMode);
          const value = exchangeValues[field.name] ?? fieldDefault(field, radioMode);
          const fieldWidthChars = Math.max(maxLength + 1, 4);

          return (
            <label
              className="entry-field"
              key={field.name}
              style={{ flex: `${fieldWidthChars} 1 ${fieldWidthChars}em` }}
            >
              <span>{field.name}</span>
              <input
                ref={(element) => {
                  if (element) {
                    exchangeInputRefs.current[field.name] = element;
                  } else {
                    delete exchangeInputRefs.current[field.name];
                  }
                }}
                type="text"
                inputMode={kind === 'NUMERIC' || kind === 'RST' ? 'numeric' : 'text'}
                value={value}
                onChange={(event) => updateExchangeField(field, event.target.value)}
                onKeyDown={(event) => handleExchangeKeyDown(event, index)}
                readOnly={field.fixed === true}
                tabIndex={field.fixed === true ? -1 : undefined}
                className={field.fixed === true ? 'fixed-field' : ''}
                maxLength={maxLength}
              />
            </label>
          );
        })}
      </div>
      <div className="function-keys">
        <div className="f-row">
          <button className="f-key">F1 S&amp;P CQ</button>
          <button className="f-key">F2 Exch</button>
          <button className="f-key">F3 Spare</button>
          <button className="f-key">F4 KBUT</button>
          <button className="f-key">F5 His Call</button>
          <button className="f-key">F6 KBUT</button>
        </div>
        <div className="f-row">
          <button className="f-key">F7 Rpt Exch</button>
          <button className="f-key">F8 Agn?</button>
          <button className="f-key">F9 Zone</button>
          <button className="f-key">F10 Spare</button>
          <button className="f-key">F11 Spare</button>
          <button className="f-key">F12 Wipe</button>
        </div>
      </div>
      <div className="command-buttons">
        <button className="cmd-btn">Call Esc Stop</button>
        <button className="cmd-btn" onClick={resetEntryFields}>Wipe</button>
        <button className="cmd-btn">UserText</button>
        <button className="cmd-btn" onClick={logContact}>Log it</button>
        <button className="cmd-btn">Edit</button>
        <button className="cmd-btn">Mark</button>
        <button className="cmd-btn">Store</button>
        <button className="cmd-btn">Spot It</button>
        <button className="cmd-btn">QRZ</button>
      </div>
      <div className="status-bar">
        <span>
          {stationCallsign} / Op: {operatorCallsign}
        </span>
      </div>
    </div>
  );
}

export default MainWindow;
