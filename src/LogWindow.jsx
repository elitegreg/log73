import React from 'react';
import './App.css';

function formatCell(column, value) {
  if (column === 'Time' && typeof value === 'number') {
    return new Intl.DateTimeFormat(undefined, {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    }).format(new Date(value * 1000));
  }

  return value ?? '';
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
              <th key={column}>{column}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {contacts.map((entry, index) => (
            <tr key={`${entry.Time ?? 'row'}-${entry.Call ?? index}`}>
              {columns.map((column) => (
                <td key={column}>{formatCell(column, entry[column])}</td>
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
