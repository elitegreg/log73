import React from 'react';
import './App.css';

const COLUMN_FIELD_MAP = {
  Freq: 'FREQ',
  Mode: 'MODE',
  Call: 'CALL',
  'RST(s)': 'RST_SENT',
  'RST(r)': 'RST_RCVD',
  Op: 'OPERATOR',
};

function epochFromLegacyQsoDateTime(entry) {
  const date = String(entry.QSO_DATE ?? '');
  const time = String(entry.TIME_ON ?? '');

  if (!/^\d{8}$/.test(date) || !/^\d{6}$/.test(time)) {
    return null;
  }

  return Math.floor(
    Date.UTC(
      Number.parseInt(date.slice(0, 4), 10),
      Number.parseInt(date.slice(4, 6), 10) - 1,
      Number.parseInt(date.slice(6, 8), 10),
      Number.parseInt(time.slice(0, 2), 10),
      Number.parseInt(time.slice(2, 4), 10),
      Number.parseInt(time.slice(4, 6), 10),
    ) / 1000,
  );
}

function qsoEpoch(entry) {
  if (typeof entry.QSO_DATE_TIME_ON === 'number') {
    return entry.QSO_DATE_TIME_ON;
  }

  if (typeof entry._time_on_epoch === 'number') {
    return entry._time_on_epoch;
  }

  if (typeof entry.Time === 'number') {
    return entry.Time;
  }

  return epochFromLegacyQsoDateTime(entry);
}

function formatDateTime(entry) {
  const epoch = qsoEpoch(entry);

  if (epoch === null) {
    return '';
  }

  const date = new Date(epoch * 1000);
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, '0');
  const day = String(date.getUTCDate()).padStart(2, '0');
  const hour = String(date.getUTCHours()).padStart(2, '0');
  const minute = String(date.getUTCMinutes()).padStart(2, '0');
  const second = String(date.getUTCSeconds()).padStart(2, '0');

  return `${year}-${month}-${day} ${hour}:${minute}:${second}`;
}

function formatFrequency(entry) {
  const frequency = entry.FREQ ?? entry.Freq;
  const parsedFrequency = typeof frequency === 'number'
    ? frequency
    : Number.parseFloat(String(frequency));

  if (!Number.isFinite(parsedFrequency)) {
    return '';
  }

  const frequencyHz = Math.abs(parsedFrequency) < 1000000
    ? parsedFrequency * 1000000
    : parsedFrequency;

  return (frequencyHz / 1000000)
    .toFixed(6)
    .replace(/0+$/, '')
    .replace(/\.$/, '');
}

function formatCell(column, entry) {
  if (column === 'Date/Time (UTC)') {
    return formatDateTime(entry);
  }

  if (column === 'Freq') {
    return formatFrequency(entry);
  }

  if (column === 'Mult' || column === 'Pts') {
    return entry[column] ?? '';
  }

  const adifField = COLUMN_FIELD_MAP[column];
  return entry[adifField] ?? entry[column] ?? '';
}

function LogWindow({ settings, contacts }) {
  const columns = settings?.qso_columns ?? [];

  return (
    <div className="log-window">
      <div className="log-title-bar">Log: {settings?.contest ?? 'Loading contest...'}</div>
      <table className="log-table">
        <colgroup>
          {columns.map((column) => (
            <col
              key={column}
              className={column === 'Date/Time (UTC)' ? 'date-time-column' : undefined}
            />
          ))}
        </colgroup>
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column}>{column}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {contacts.map((entry, index) => (
            <tr
              key={entry._id ?? entry._client_id ?? `${entry.QSO_DATE_TIME_ON ?? entry.TIME_ON ?? entry.Time ?? 'row'}-${entry.CALL ?? entry.Call ?? index}`}
              className={entry._status !== 'Committed' ? 'uncommitted-contact' : undefined}
            >
              {columns.map((column) => (
                <td key={column}>{formatCell(column, entry)}</td>
              ))}
            </tr>
          ))}
          {contacts.length === 0 && (
            <tr>
              <td colSpan={Math.max(columns.length, 1)} className="empty-log">
                No contacts loaded.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

export default LogWindow;
