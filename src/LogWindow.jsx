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

function epochFromQsoDateTime(entry) {
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

function formatDate(entry) {
  const qsoDate = String(entry.QSO_DATE ?? '');

  if (/^\d{8}$/.test(qsoDate)) {
    return `${qsoDate.slice(0, 4)}-${qsoDate.slice(4, 6)}-${qsoDate.slice(6, 8)}`;
  }

  const epoch = typeof entry._time_on_epoch === 'number'
    ? entry._time_on_epoch
    : typeof entry.Time === 'number'
      ? entry.Time
      : null;

  if (epoch === null) {
    return '';
  }

  return new Intl.DateTimeFormat(undefined, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    timeZone: 'UTC',
  }).format(new Date(epoch * 1000));
}

function formatTime(entry) {
  const epoch = typeof entry._time_on_epoch === 'number'
    ? entry._time_on_epoch
    : epochFromQsoDateTime(entry);

  if (epoch === null) {
    return '';
  }

  return new Intl.DateTimeFormat(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
    timeZone: 'UTC',
  }).format(new Date(epoch * 1000));
}

function formatCell(column, entry) {
  if (column === 'Date') {
    return formatDate(entry);
  }

  if (column === 'Time') {
    return typeof entry.Time === 'number'
      ? new Intl.DateTimeFormat(undefined, {
          hour: '2-digit',
          minute: '2-digit',
          second: '2-digit',
          hour12: false,
          timeZone: 'UTC',
        }).format(new Date(entry.Time * 1000))
      : formatTime(entry);
  }

  if (column === 'Mult' || column === 'Pts') {
    return entry[column] ?? '';
  }

  const adifField = COLUMN_FIELD_MAP[column];
  return entry[adifField] ?? entry[column] ?? '';
}

function headerLabel(column) {
  return column === 'Time' ? 'Time (UTC)' : column;
}

function LogWindow({ settings, contacts }) {
  const columns = settings?.qso_columns ?? [];

  return (
    <div className="log-window">
      <div className="log-title-bar">Log: {settings?.contest ?? 'Loading contest...'}</div>
      <table className="log-table">
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column}>{headerLabel(column)}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {contacts.map((entry, index) => (
            <tr
              key={entry._id ?? `${entry.TIME_ON ?? entry.Time ?? 'row'}-${entry.CALL ?? entry.Call ?? index}`}
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
