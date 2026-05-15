import React, { useEffect, useRef, useState } from 'react';
import './App.css';

export const STATION_CALLSIGN = 'NG4M';
const RADIO_MODE = 'CW';
const RADIO_FREQ = 14025;

function parseFieldType(type = '') {
  const [rawKind = 'STRING', length = '8'] = type.split(':');
  const kind = rawKind.toUpperCase();
  const maxLength =
    kind === 'RST'
      ? RADIO_MODE === 'CW'
        ? 3
        : 2
      : Number.parseInt(length, 10) || 8;
  return { kind, maxLength };
}

function sanitizeRST(value) {
  const maxLength = RADIO_MODE === 'CW' ? 3 : 2;
  let nextValue = value.replace(/[^1-9]/g, '').slice(0, maxLength);

  while (nextValue.length > 0 && !/^[1-5]$/.test(nextValue[0])) {
    nextValue = nextValue.slice(1);
  }

  return nextValue;
}

function fieldDefault(field) {
  if (field.default === undefined || field.default === null) {
    return '';
  }

  const value = String(field.default);
  return parseFieldType(field.type).kind === 'RST' ? sanitizeRST(value) : value;
}

function MainWindow({ settings, operatorCallsign }) {
  const [callSign, setCallSign] = useState('');
  const [exchangeValues, setExchangeValues] = useState({});
  const callSignRef = useRef(null);

  useEffect(() => {
    if (!settings?.exchange) {
      return;
    }

    const defaults = Object.fromEntries(
      settings.exchange.map((field) => [field.name, fieldDefault(field)]),
    );
    setExchangeValues(defaults);
  }, [settings]);

  function updateExchangeField(field, value) {
    const { kind, maxLength } = parseFieldType(field.type);
    let nextValue = value.slice(0, maxLength);

    if (kind === 'RST') {
      nextValue = sanitizeRST(nextValue);
    } else if (kind === 'NUMERIC') {
      nextValue = nextValue.replace(/\D/g, '');
    } else {
      nextValue = nextValue.toUpperCase();
    }

    setExchangeValues((current) => ({ ...current, [field.name]: nextValue }));
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
        Mode: {RADIO_MODE}, Freq: {RADIO_FREQ} -{' '}
        {settings?.contest ?? 'Loading...'}
      </div>
      <div className="menu-bar">
        <span>File</span>
        <span>Edit</span>
        <span>View</span>
        <span>Tools</span>
        <span>Config</span>
        <span>Window</span>
        <span>Help</span>
      </div>
      <div className="entry-fields">
        <label className="entry-field">
          <span>Callsign</span>
          <input
            ref={callSignRef}
            type="text"
            value={callSign}
            onChange={(event) =>
              setCallSign(event.target.value.toUpperCase().slice(0, 12))
            }
            className="callsign"
            maxLength={12}
          />
        </label>
        {settings?.exchange?.map((field, index) => {
          const { kind, maxLength } = parseFieldType(field.type);
          const value = exchangeValues[field.name] ?? fieldDefault(field);
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
      <div className="history">Call history appears here when enabled.</div>
      <div className="status-bar">
        <span>
          {STATION_CALLSIGN} / Op: {operatorCallsign}
        </span>
      </div>
    </div>
  );
}

export default MainWindow;
