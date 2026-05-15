import React, { useEffect, useRef, useState } from 'react';
import './App.css';

export const STATION_CALLSIGN = 'NG4M';
const MODE_OPTIONS = ['CW', 'SSB', 'FM', 'AM'];
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

function parseFieldType(type = '', radioMode = 'CW') {
  const [rawKind = 'STRING', length = '8'] = type.split(':');
  const kind = rawKind.toUpperCase();
  const maxLength =
    kind === 'RST'
      ? radioMode === 'CW'
        ? 3
        : 2
      : Number.parseInt(length, 10) || 8;
  return { kind, maxLength };
}

function sanitizeRST(value, radioMode = 'CW') {
  const maxLength = radioMode === 'CW' ? 3 : 2;
  let nextValue = value.replace(/[^1-9]/g, '').slice(0, maxLength);

  while (nextValue.length > 0 && !/^[1-5]$/.test(nextValue[0])) {
    nextValue = nextValue.slice(1);
  }

  return nextValue;
}

function fieldDefault(field, radioMode) {
  if (field.default === undefined || field.default === null) {
    return '';
  }

  const value = String(field.default);
  return parseFieldType(field.type, radioMode).kind === 'RST'
    ? sanitizeRST(value, radioMode)
    : value;
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

function MainWindow({
  settings,
  operatorCallsign,
  radioState,
  onSetRadioFrequency,
  onSetRadioMode,
}) {
  const [callSign, setCallSign] = useState('');
  const [exchangeValues, setExchangeValues] = useState({});
  const callSignRef = useRef(null);
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
    if (!settings?.exchange) {
      return;
    }

    const defaults = Object.fromEntries(
      settings.exchange.map((field) => [field.name, fieldDefault(field, radioMode)]),
    );
    setExchangeValues(defaults);
  }, [settings, radioMode]);

  function updateExchangeField(field, value) {
    const { kind, maxLength } = parseFieldType(field.type, radioMode);
    let nextValue = value.slice(0, maxLength);

    if (kind === 'RST') {
      nextValue = sanitizeRST(nextValue, radioMode);
    } else if (kind === 'NUMERIC') {
      nextValue = nextValue.replace(/\D/g, '');
    } else {
      nextValue = nextValue.toUpperCase();
    }

    setExchangeValues((current) => ({ ...current, [field.name]: nextValue }));
  }

  function handleCallsignChange(event) {
    setCallSign(event.target.value.toUpperCase().slice(0, 12));
  }

  function handleCallsignKeyDown(event) {
    const value = callSign.trim();

    if (event.key === 'Enter' && isFrequencyInput(value)) {
      event.preventDefault();
      onSetRadioFrequency?.(Math.round(Number.parseFloat(value) * 1000));
      setCallSign('');
    }
  }

  function handleBandChange(event) {
    const selectedBand = bandByMeters(Number.parseInt(event.target.value, 10));

    if (selectedBand) {
      onSetRadioFrequency?.(selectedBand.lowerHz);
    }
  }

  function handleExchangeKeyDown(event, index) {
    const editableFields = settings.exchange.filter((field) => field.fixed !== true);
    const lastEditableField = editableFields[editableFields.length - 1];

    if (
      event.key === 'Tab' &&
      !event.shiftKey &&
      settings.exchange[index]?.name === lastEditableField?.name
    ) {
      event.preventDefault();
      callSignRef.current?.focus();
    }
  }

  return (
    <div className="window">
      <div className="title-bar">
        Mode: {radioMode}, Freq: {formatFrequency(radioFrequencyHz)} -{' '}
        {settings?.contest ?? 'Loading...'}
      </div>
      <div className="radio-controls">
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
      </div>
      <div className="entry-fields">
        <label className="entry-field">
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
          const width = `${Math.max(maxLength + 1, 4)}ch`;

          return (
            <label className="entry-field" key={field.name}>
              <span>{field.name}</span>
              <input
                type="text"
                inputMode={kind === 'NUMERIC' || kind === 'RST' ? 'numeric' : 'text'}
                value={value}
                onChange={(event) => updateExchangeField(field, event.target.value)}
                onKeyDown={(event) => handleExchangeKeyDown(event, index)}
                readOnly={field.fixed === true}
                tabIndex={field.fixed === true ? -1 : undefined}
                className={field.fixed === true ? 'fixed-field' : ''}
                maxLength={maxLength}
                style={{ width }}
              />
            </label>
          );
        })}
      </div>
      <div className="function-keys">
        <div className="f-row">
          <button className="f-key active">F1 S&amp;P CQ</button>
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
        <button className="cmd-btn">Wipe</button>
        <button className="cmd-btn">UserText</button>
        <button className="cmd-btn">Log it</button>
        <button className="cmd-btn">Edit</button>
        <button className="cmd-btn">Mark</button>
        <button className="cmd-btn">Store</button>
        <button className="cmd-btn">Spot It</button>
        <button className="cmd-btn">QRZ</button>
      </div>
      <div className="status-bar">
        <span>
          {STATION_CALLSIGN} / Op: {operatorCallsign}
        </span>
      </div>
    </div>
  );
}

export default MainWindow;
