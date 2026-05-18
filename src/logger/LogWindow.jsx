import React, { useEffect, useMemo, useRef, useState } from 'react';
import { sanitizeCallsign, sanitizeExchangeValue } from '../domain/contactFields';


const READ_ONLY_COLUMNS = new Set(['Mult', 'Pts']);

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

function formatFrequency(entry, field = 'FREQ') {
  const frequency = entry[field] ?? entry.FREQ ?? entry.Freq;
  const parsedFrequency = typeof frequency === 'number'
    ? frequency
    : Number.parseFloat(String(frequency));

  if (!Number.isFinite(parsedFrequency)) {
    return '';
  }

  const frequencyHz = Math.abs(parsedFrequency) < 1000000
    ? parsedFrequency * 1000000
    : parsedFrequency;

  return (frequencyHz / 1000)
    .toFixed(3)
    .replace(/0+$/, '')
    .replace(/\.$/, '');
}

function fieldMapFromSettings(settings) {
  const fieldMap = { ...(settings?.qso_column_fields ?? {}) };

  for (const field of settings?.exchange ?? []) {
    if (field.name && field.adif) fieldMap[field.name] = field.adif;
  }

  return fieldMap;
}

function formatCell(column, entry, columnFieldMap) {
  if (column === 'Date/Time (UTC)') {
    return formatDateTime(entry);
  }

  if (column === 'Freq') {
    return formatFrequency(entry, columnFieldMap[column]);
  }

  if (column === 'Mult' || column === 'Pts') {
    return entry[column] ?? '';
  }

  const adifField = columnFieldMap[column];
  return entry[adifField] ?? entry[column] ?? '';
}

function contactKey(entry, index) {
  return String(entry._id ?? entry._client_id ?? `${entry.QSO_DATE_TIME_ON ?? entry.TIME_ON ?? entry.Time ?? 'row'}-${entry.CALL ?? entry.Call ?? index}`);
}

function editableFieldForColumn(column, columnFieldMap) {
  if (READ_ONLY_COLUMNS.has(column)) return null;
  if (column === 'Date/Time (UTC)') return 'QSO_DATE_TIME_ON';
  return columnFieldMap[column] ?? column;
}

function parseDateTimeUtc(value) {
  const match = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})$/.exec(value.trim());
  if (!match) return null;

  const [, yearText, monthText, dayText, hourText, minuteText, secondText] = match;
  const year = Number.parseInt(yearText, 10);
  const month = Number.parseInt(monthText, 10);
  const day = Number.parseInt(dayText, 10);
  const hour = Number.parseInt(hourText, 10);
  const minute = Number.parseInt(minuteText, 10);
  const second = Number.parseInt(secondText, 10);
  const milliseconds = Date.UTC(year, month - 1, day, hour, minute, second);
  const date = new Date(milliseconds);

  if (
    date.getUTCFullYear() !== year
    || date.getUTCMonth() !== month - 1
    || date.getUTCDate() !== day
    || date.getUTCHours() !== hour
    || date.getUTCMinutes() !== minute
    || date.getUTCSeconds() !== second
  ) {
    return null;
  }

  return Math.floor(milliseconds / 1000);
}

function exchangeFieldForColumn(settings, column) {
  return (settings?.exchange ?? []).find((field) => field.name === column);
}

function sanitizeUpdateInput(settings, column, value, radioMode) {
  const exchangeField = exchangeFieldForColumn(settings, column);
  if (exchangeField) return sanitizeExchangeValue(exchangeField, value, radioMode);
  if (column === 'Call') return sanitizeCallsign(value);
  if (column === 'Mode') return value.toUpperCase();
  return value;
}

function parseUpdateValue(settings, column, value, radioMode) {
  if (column === 'Date/Time (UTC)') {
    const epoch = parseDateTimeUtc(value);
    if (epoch === null) {
      return { ok: false, error: 'Enter date/time as YYYY-MM-DD HH:MM:SS in UTC.' };
    }

    return { ok: true, value: epoch };
  }

  if (column === 'Freq') {
    const parsedFrequency = Number.parseFloat(String(value));
    if (!Number.isFinite(parsedFrequency) || parsedFrequency <= 0) {
      return { ok: false, error: 'Enter a valid frequency.' };
    }

    return {
      ok: true,
      value: Math.round(parsedFrequency * 1000),
    };
  }

  if (column === 'Mode') {
    const mode = value.trim().toUpperCase();
    if ((settings?.allowed_modes ?? []).length > 0 && !settings.allowed_modes.includes(mode)) {
      return { ok: false, error: `Enter one of: ${settings.allowed_modes.join(', ')}.` };
    }

    return { ok: true, value: mode };
  }

  return { ok: true, value: sanitizeUpdateInput(settings, column, value, radioMode).trim() };
}

function LogWindow({ settings, contacts, log, radioMode = 'CW', onDeleteContacts, onUpdateContacts }) {
  const columns = settings?.qso_columns ?? [];
  const columnFieldMap = useMemo(() => fieldMapFromSettings(settings), [settings]);
  const [selectedKeys, setSelectedKeys] = useState(() => new Set());
  const [contextMenu, setContextMenu] = useState(null);
  const [editingCell, setEditingCell] = useState(null);
  const lastSelectedIndexRef = useRef(null);
  const inputRef = useRef(null);
  const editingCellKey = editingCell?.key;
  const editingCellColumn = editingCell?.column;

  useEffect(() => {
    const validKeys = new Set(contacts.map(contactKey));
    setSelectedKeys((currentKeys) => {
      const nextKeys = new Set([...currentKeys].filter((key) => validKeys.has(key)));
      return nextKeys.size === currentKeys.size ? currentKeys : nextKeys;
    });
  }, [contacts]);

  useEffect(() => {
    function closeContextMenu() { setContextMenu(null); }
    window.addEventListener('click', closeContextMenu);
    window.addEventListener('keydown', closeContextMenu);
    return () => {
      window.removeEventListener('click', closeContextMenu);
      window.removeEventListener('keydown', closeContextMenu);
    };
  }, []);

  useEffect(() => {
    if (!editingCellKey || !editingCellColumn) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [editingCellKey, editingCellColumn]);

  function selectedContacts() {
    return contacts.filter((entry, index) => selectedKeys.has(contactKey(entry, index)));
  }

  function selectRow(event, index, key) {
    setContextMenu(null);
    setEditingCell(null);

    if (event.shiftKey && lastSelectedIndexRef.current !== null) {
      event.preventDefault();
      const start = Math.min(lastSelectedIndexRef.current, index);
      const end = Math.max(lastSelectedIndexRef.current, index);
      setSelectedKeys(new Set(contacts.slice(start, end + 1).map(contactKey)));
      return;
    }

    lastSelectedIndexRef.current = index;

    if (event.ctrlKey || event.metaKey) {
      setSelectedKeys((currentKeys) => {
        const nextKeys = new Set(currentKeys);
        if (nextKeys.has(key)) nextKeys.delete(key);
        else nextKeys.add(key);
        return nextKeys;
      });
      return;
    }

    setSelectedKeys(new Set([key]));
  }

  function openContextMenu(event, entry, index, column) {
    event.preventDefault();
    const key = contactKey(entry, index);
    let menuSelectedKeys = selectedKeys;

    if (!selectedKeys.has(key)) {
      menuSelectedKeys = new Set([key]);
      setSelectedKeys(menuSelectedKeys);
      lastSelectedIndexRef.current = index;
    }

    setEditingCell(null);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      contactKey: key,
      column,
      selectedCount: menuSelectedKeys.size,
    });
  }

  function beginUpdate() {
    if (!contextMenu) return;
    const field = editableFieldForColumn(contextMenu.column, columnFieldMap);
    if (!field) return;
    const contactIndex = contacts.findIndex((entry, index) => contactKey(entry, index) === contextMenu.contactKey);
    if (contactIndex === -1) return;

    setEditingCell({
      key: contextMenu.contactKey,
      column: contextMenu.column,
      value: String(formatCell(contextMenu.column, contacts[contactIndex], columnFieldMap)),
    });
    setContextMenu(null);
  }

  function deleteSelected() {
    const contactsToDelete = selectedContacts();
    setContextMenu(null);
    onDeleteContacts?.(contactsToDelete);
  }

  function finishUpdate() {
    if (!editingCell) return;
    const field = editableFieldForColumn(editingCell.column, columnFieldMap);
    if (!field) return;

    const parsed = parseUpdateValue(settings, editingCell.column, editingCell.value, radioMode);
    if (!parsed.ok) {
      window.alert(parsed.error);
      inputRef.current?.focus();
      return;
    }

    const contactsToUpdate = selectedContacts();
    onUpdateContacts?.(contactsToUpdate, field, parsed.value);
    setEditingCell(null);
  }

  return (
    <div className="log-window">
      <div className="log-title-bar">Log: {log?.name ?? 'Loading log...'} - {settings?.contest ?? 'Loading contest...'}</div>
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
          {contacts.map((entry, index) => {
            const key = contactKey(entry, index);
            const isSelected = selectedKeys.has(key);
            return (
              <tr
                key={key}
                className={`${entry._status !== 'Committed' ? 'uncommitted-contact' : ''}${isSelected ? ' selected-contact' : ''}`.trim() || undefined}
                onClick={(event) => selectRow(event, index, key)}
              >
                {columns.map((column) => {
                  const isEditing = editingCell?.key === key && editingCell.column === column;
                  return (
                    <td key={column} onContextMenu={(event) => openContextMenu(event, entry, index, column)}>
                      {isEditing ? (
                        <input
                          ref={inputRef}
                          className="log-cell-editor"
                          value={editingCell.value}
                          onChange={(event) => setEditingCell({
                            ...editingCell,
                            value: sanitizeUpdateInput(settings, editingCell.column, event.target.value, radioMode),
                          })}
                          onClick={(event) => event.stopPropagation()}
                          onKeyDown={(event) => {
                            if (event.key === 'Enter') {
                              event.preventDefault();
                              finishUpdate();
                            } else if (event.key === 'Escape') {
                              event.preventDefault();
                              setEditingCell(null);
                            }
                          }}
                        />
                      ) : formatCell(column, entry, columnFieldMap)}
                    </td>
                  );
                })}
              </tr>
            );
          })}
          {contacts.length === 0 && (
            <tr>
              <td colSpan={Math.max(columns.length, 1)} className="empty-log">
                No contacts loaded.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      {contextMenu && (
        <div
          className="log-context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            disabled={!editableFieldForColumn(contextMenu.column, columnFieldMap)}
            onClick={beginUpdate}
          >
            Update selected {contextMenu.selectedCount === 1 ? 'QSO' : `${contextMenu.selectedCount} QSOs`}
          </button>
          <button type="button" onClick={deleteSelected}>
            Delete selected {contextMenu.selectedCount === 1 ? 'QSO' : `${contextMenu.selectedCount} QSOs`}
          </button>
        </div>
      )}
    </div>
  );
}

export default LogWindow;
