import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import {
  parseFieldType,
  sanitizeCallsign,
  sanitizeExchangeValue,
} from '../domain/contactFields';
import { adifModeForLoggerMode } from '../domain/modes';
import { validateExchangeField } from '../domain/validation';
import { useNotifications } from '../lib/notificationsContext';

const READ_ONLY_COLUMNS = new Set(['Mult', 'Pts']);
const COLUMN_PADDING_CHARS = 2;
const FIXED_COLUMN_WIDTHS = {
  'Date/Time (UTC)': 19,
  Freq: 7,
  Mode: 3,
  Call: 12,
  Mult: 2,
  Pts: 2,
  Op: 12,
};
const VIRTUAL_ROW_HEIGHT_PX = 22;
const VIRTUAL_OVERSCAN_ROWS = 8;
const LOAD_MORE_THRESHOLD_PX = 120;

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
  const parsedFrequency =
    typeof frequency === 'number'
      ? frequency
      : Number.parseFloat(String(frequency));

  if (!Number.isFinite(parsedFrequency)) {
    return '';
  }

  const frequencyHz =
    Math.abs(parsedFrequency) < 1000000
      ? parsedFrequency * 1000000
      : parsedFrequency;

  return (frequencyHz / 1000).toFixed(3).replace(/0+$/, '').replace(/\.$/, '');
}

function fieldMapFromSettings(settings) {
  const fieldMap = { ...(settings?.qso_column_fields ?? {}) };

  for (const field of settings?.exchange ?? []) {
    if (field.name && field.adif) fieldMap[field.name] = field.adif;
  }

  return fieldMap;
}

function columnWidthChars(settings, column, radioMode) {
  const headerWidth = String(column).length;
  let dataWidth = FIXED_COLUMN_WIDTHS[column];

  const exchangeField = exchangeFieldForColumn(settings, column);
  if (!dataWidth && exchangeField) {
    dataWidth = parseFieldType(exchangeField.type, radioMode).maxLength;
  }

  return Math.max(dataWidth ?? 4, headerWidth, 4);
}

function columnWidthPercent(settings, column, radioMode, columns) {
  const totalWidthChars = columns.reduce(
    (total, currentColumn) =>
      total +
      columnWidthChars(settings, currentColumn, radioMode) +
      COLUMN_PADDING_CHARS,
    0,
  );
  const widthChars =
    columnWidthChars(settings, column, radioMode) + COLUMN_PADDING_CHARS;
  return `${(widthChars / Math.max(totalWidthChars, 1)) * 100}%`;
}

function columnWidthStyle(settings, column, radioMode, columns) {
  return { width: columnWidthPercent(settings, column, radioMode, columns) };
}

function exchangeValueForColumn(settings, column, entry, columnFieldMap) {
  const exchangeField = exchangeFieldForColumn(settings, column);
  if (!exchangeField) return null;
  const adifField = columnFieldMap[column] ?? exchangeField.adif;
  return entry[adifField] ?? entry[column] ?? '';
}

function contactMode(entry, fallbackMode = 'CW') {
  return String(entry.MODE ?? entry.Mode ?? fallbackMode).toUpperCase();
}

function cellValidation(settings, column, entry, columnFieldMap, radioMode) {
  const exchangeField = exchangeFieldForColumn(settings, column);
  if (!exchangeField || exchangeField.is_sent) return { ok: true, error: '' };
  return validateExchangeField(
    exchangeField,
    exchangeValueForColumn(settings, column, entry, columnFieldMap),
    contactMode(entry, radioMode),
  );
}

function formatCell(column, entry, columnFieldMap) {
  if (column === 'Date/Time (UTC)') {
    return formatDateTime(entry);
  }

  if (column === 'Freq') {
    return formatFrequency(entry, columnFieldMap[column]);
  }

  if (column === 'Mult') {
    return entry._mult ?? entry[column] ?? '';
  }

  if (column === 'Pts') {
    return entry._pts ?? entry[column] ?? '';
  }

  const adifField = columnFieldMap[column];
  return entry[adifField] ?? entry[column] ?? '';
}

function contactKey(entry, index) {
  if (entry?._client_id) return `client:${entry._client_id}`;
  if (entry?._id !== undefined && entry?._id !== null) return `id:${entry._id}`;

  return `row:${entry.QSO_DATE_TIME_ON ?? entry.TIME_ON ?? entry.Time ?? 'row'}-${entry.CALL ?? entry.Call ?? index}`;
}

function contactRowClassName(entry, isSelected) {
  const classes = [];
  if (entry._status === 'Failed') classes.push('failed-contact');
  else if (entry._status !== 'Committed') classes.push('uncommitted-contact');
  if (isSelected) classes.push('selected-contact');
  return classes.join(' ') || undefined;
}

function contactRowTitle(entry) {
  if (entry._status !== 'Failed') return undefined;
  return entry._error
    ? `Contact upload failed: ${entry._error}`
    : 'Contact upload failed.';
}

function isSentSerialExchangeField(field) {
  return (
    field?.is_sent === true && parseFieldType(field?.type).kind === 'SERIAL'
  );
}

function editableFieldForColumn(settings, column, columnFieldMap) {
  if (READ_ONLY_COLUMNS.has(column)) return null;
  if (isSentSerialExchangeField(exchangeFieldForColumn(settings, column))) {
    return null;
  }
  if (column === 'Date/Time (UTC)') return 'QSO_DATE_TIME_ON';
  return columnFieldMap[column] ?? column;
}

function parseDateTimeUtc(value) {
  const match = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})$/.exec(
    value.trim(),
  );
  if (!match) return null;

  const [, yearText, monthText, dayText, hourText, minuteText, secondText] =
    match;
  const year = Number.parseInt(yearText, 10);
  const month = Number.parseInt(monthText, 10);
  const day = Number.parseInt(dayText, 10);
  const hour = Number.parseInt(hourText, 10);
  const minute = Number.parseInt(minuteText, 10);
  const second = Number.parseInt(secondText, 10);
  const milliseconds = Date.UTC(year, month - 1, day, hour, minute, second);
  const date = new Date(milliseconds);

  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day ||
    date.getUTCHours() !== hour ||
    date.getUTCMinutes() !== minute ||
    date.getUTCSeconds() !== second
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
  if (exchangeField)
    return sanitizeExchangeValue(exchangeField, value, radioMode);
  if (column === 'Call') return sanitizeCallsign(value);
  if (column === 'Mode') return String(value).toUpperCase();
  return String(value).toUpperCase();
}

function parseUpdateValue(settings, column, value, radioMode, entry = null) {
  if (column === 'Date/Time (UTC)') {
    const epoch = parseDateTimeUtc(value);
    if (epoch === null) {
      return {
        ok: false,
        error: 'Enter date/time as YYYY-MM-DD HH:MM:SS in UTC.',
      };
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
    const mode = adifModeForLoggerMode(value);
    if (
      (settings?.allowed_modes ?? []).length > 0 &&
      !settings.allowed_modes.some(
        (allowedMode) => String(allowedMode).trim().toUpperCase() === mode,
      )
    ) {
      return {
        ok: false,
        error: `Enter one of: ${settings.allowed_modes.join(', ')}.`,
      };
    }

    return { ok: true, value: mode };
  }

  const validationMode = entry ? contactMode(entry, radioMode) : radioMode;
  const sanitizedValue = sanitizeUpdateInput(
    settings,
    column,
    value,
    validationMode,
  ).trim();
  const exchangeField = exchangeFieldForColumn(settings, column);
  if (exchangeField && !exchangeField.is_sent) {
    const validation = validateExchangeField(
      exchangeField,
      sanitizedValue,
      validationMode,
    );
    if (!validation.ok) return { ok: false, error: validation.error };
  }

  return { ok: true, value: sanitizedValue };
}

function contextMenuPositionStyle(contextMenu) {
  const menuWidth = 190;
  const menuHeight = 72;
  const viewportWidth =
    typeof window === 'undefined' ? menuWidth : window.innerWidth;
  const viewportHeight =
    typeof window === 'undefined' ? menuHeight : window.innerHeight;

  return {
    left: Math.max(0, Math.min(contextMenu.x, viewportWidth - menuWidth)),
    top: Math.max(0, Math.min(contextMenu.y, viewportHeight - menuHeight)),
  };
}

function LogWindow({
  settings,
  contacts,
  log,
  contactsLoadState,
  radioMode = 'CW',
  onDeleteContacts,
  onUpdateContacts,
  hasMoreContacts = false,
  isLoadingMoreContacts = false,
  onLoadMoreContacts,
}) {
  const { notifyError } = useNotifications();
  const columns = settings?.qso_columns ?? [];
  const columnFieldMap = useMemo(
    () => fieldMapFromSettings(settings),
    [settings],
  );
  const [selectedKeys, setSelectedKeys] = useState(() => new Set());
  const [contextMenu, setContextMenu] = useState(null);
  const [editingCell, setEditingCell] = useState(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(230);
  const lastSelectedIndexRef = useRef(null);
  const inputRef = useRef(null);
  const scrollContainerRef = useRef(null);
  const editingCellKey = editingCell?.key;
  const editingCellColumn = editingCell?.column;
  const contactsLoadMessage =
    contactsLoadState === 'initial-loading'
      ? 'Loading contacts...'
      : contactsLoadState === 'refreshing'
        ? 'Refreshing contacts...'
        : contactsLoadState === 'retrying'
          ? 'Retrying contact load...'
          : '';
  const visibleRowCount = Math.max(
    1,
    Math.ceil(viewportHeight / VIRTUAL_ROW_HEIGHT_PX),
  );
  const startIndex = Math.max(
    0,
    Math.floor(scrollTop / VIRTUAL_ROW_HEIGHT_PX) - VIRTUAL_OVERSCAN_ROWS,
  );
  const endIndex = Math.min(
    contacts.length,
    startIndex + visibleRowCount + VIRTUAL_OVERSCAN_ROWS * 2,
  );
  const visibleContacts = contacts.slice(startIndex, endIndex);
  const topSpacerHeight = startIndex * VIRTUAL_ROW_HEIGHT_PX;
  const bottomSpacerHeight = Math.max(
    0,
    (contacts.length - endIndex) * VIRTUAL_ROW_HEIGHT_PX,
  );

  useEffect(() => {
    const validKeys = new Set(contacts.map(contactKey));
    setSelectedKeys((currentKeys) => {
      const nextKeys = new Set(
        [...currentKeys].filter((key) => validKeys.has(key)),
      );
      return nextKeys.size === currentKeys.size ? currentKeys : nextKeys;
    });
  }, [contacts]);

  useEffect(() => {
    function closeContextMenu() {
      setContextMenu(null);
    }
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

  useEffect(() => {
    function updateViewportHeight() {
      setViewportHeight(scrollContainerRef.current?.clientHeight ?? 230);
    }

    updateViewportHeight();
    window.addEventListener('resize', updateViewportHeight);
    return () => window.removeEventListener('resize', updateViewportHeight);
  }, []);

  function maybeLoadMoreContacts(container) {
    if (
      !container ||
      !hasMoreContacts ||
      isLoadingMoreContacts ||
      typeof onLoadMoreContacts !== 'function'
    ) {
      return;
    }

    const remainingPx =
      container.scrollHeight - (container.scrollTop + container.clientHeight);
    if (remainingPx <= LOAD_MORE_THRESHOLD_PX) {
      onLoadMoreContacts();
    }
  }

  function handleTableScroll(event) {
    const container = event.currentTarget;
    setScrollTop(container.scrollTop);
    maybeLoadMoreContacts(container);
  }

  function selectedContacts() {
    return contacts.filter((entry, index) =>
      selectedKeys.has(contactKey(entry, index)),
    );
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
    const field = editableFieldForColumn(
      settings,
      contextMenu.column,
      columnFieldMap,
    );
    if (!field) return;
    const contactIndex = contacts.findIndex(
      (entry, index) => contactKey(entry, index) === contextMenu.contactKey,
    );
    if (contactIndex === -1) return;

    setEditingCell({
      key: contextMenu.contactKey,
      column: contextMenu.column,
      value: String(
        formatCell(contextMenu.column, contacts[contactIndex], columnFieldMap),
      ),
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
    const field = editableFieldForColumn(
      settings,
      editingCell.column,
      columnFieldMap,
    );
    if (!field) return;

    const contactIndex = contacts.findIndex(
      (entry, index) => contactKey(entry, index) === editingCell.key,
    );
    const editingContact = contactIndex === -1 ? null : contacts[contactIndex];
    const parsed = parseUpdateValue(
      settings,
      editingCell.column,
      editingCell.value,
      radioMode,
      editingContact,
    );
    if (!parsed.ok) {
      notifyError(parsed.error, {
        dedupeKey: `LogWindow.inlineEdit:${editingCell.column}:${parsed.error}`,
      });
      inputRef.current?.focus();
      return;
    }

    const contactsToUpdate = selectedContacts();
    onUpdateContacts?.(contactsToUpdate, field, parsed.value);
    setEditingCell(null);
  }

  return (
    <div className="log-window">
      <div className="log-title-bar">
        <div className="log-title-main">
          Log: {log?.name ?? 'Loading log...'} -{' '}
          {settings?.contest ?? 'Loading contest...'}
          {contactsLoadMessage ? (
            <span className="log-title-status"> ({contactsLoadMessage})</span>
          ) : null}
        </div>
      </div>
      <div
        className="log-table-scroll"
        ref={scrollContainerRef}
        onScroll={handleTableScroll}
      >
        <table className="log-table">
          <colgroup>
            {columns.map((column) => (
              <col
                key={column}
                style={columnWidthStyle(settings, column, radioMode, columns)}
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
            {contacts.length === 0 ? (
              <tr>
                <td colSpan={Math.max(columns.length, 1)} className="empty-log">
                  {contactsLoadMessage || 'No contacts loaded.'}
                </td>
              </tr>
            ) : (
              <>
                {topSpacerHeight > 0 ? (
                  <tr className="virtual-spacer" aria-hidden>
                    <td
                      colSpan={Math.max(columns.length, 1)}
                      style={{ height: `${topSpacerHeight}px` }}
                    />
                  </tr>
                ) : null}
                {visibleContacts.map((entry, rowOffset) => {
                  const index = startIndex + rowOffset;
                  const key = contactKey(entry, index);
                  const isSelected = selectedKeys.has(key);
                  return (
                    <tr
                      key={key}
                      className={contactRowClassName(entry, isSelected)}
                      title={contactRowTitle(entry)}
                      onClick={(event) => selectRow(event, index, key)}
                    >
                      {columns.map((column) => {
                        const isEditing =
                          editingCell?.key === key &&
                          editingCell.column === column;
                        const validation = cellValidation(
                          settings,
                          column,
                          entry,
                          columnFieldMap,
                          radioMode,
                        );
                        return (
                          <td
                            key={column}
                            className={
                              validation.ok ? undefined : 'invalid-cell'
                            }
                            title={validation.ok ? undefined : validation.error}
                            onContextMenu={(event) =>
                              openContextMenu(event, entry, index, column)
                            }
                          >
                            {isEditing ? (
                              <input
                                ref={inputRef}
                                className={`log-cell-editor ${parseUpdateValue(settings, editingCell.column, editingCell.value, radioMode, entry).ok ? '' : 'invalid-field'}`.trim()}
                                value={editingCell.value}
                                onChange={(event) =>
                                  setEditingCell({
                                    ...editingCell,
                                    value: sanitizeUpdateInput(
                                      settings,
                                      editingCell.column,
                                      event.target.value,
                                      radioMode,
                                    ),
                                  })
                                }
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
                            ) : (
                              formatCell(column, entry, columnFieldMap)
                            )}
                          </td>
                        );
                      })}
                    </tr>
                  );
                })}
                {bottomSpacerHeight > 0 ? (
                  <tr className="virtual-spacer" aria-hidden>
                    <td
                      colSpan={Math.max(columns.length, 1)}
                      style={{ height: `${bottomSpacerHeight}px` }}
                    />
                  </tr>
                ) : null}
                {isLoadingMoreContacts ? (
                  <tr className="loading-more-row" aria-live="polite">
                    <td colSpan={Math.max(columns.length, 1)}>
                      Loading more contacts...
                    </td>
                  </tr>
                ) : null}
              </>
            )}
          </tbody>
        </table>
      </div>
      {contextMenu
        ? createPortal(
            <div
              className="log-context-menu"
              style={contextMenuPositionStyle(contextMenu)}
              onClick={(event) => event.stopPropagation()}
            >
              <button
                type="button"
                disabled={
                  !editableFieldForColumn(
                    settings,
                    contextMenu.column,
                    columnFieldMap,
                  )
                }
                onClick={beginUpdate}
              >
                Update selected{' '}
                {contextMenu.selectedCount === 1
                  ? 'QSO'
                  : `${contextMenu.selectedCount} QSOs`}
              </button>
              <button type="button" onClick={deleteSelected}>
                Delete selected{' '}
                {contextMenu.selectedCount === 1
                  ? 'QSO'
                  : `${contextMenu.selectedCount} QSOs`}
              </button>
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}

export default LogWindow;
