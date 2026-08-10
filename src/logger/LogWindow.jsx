import React, { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { parseFieldInput } from '../domain/contactFields';
import {
  epochFromLegacyQsoDateTime,
  formatUtcDateTime,
  parseUtcDateTime,
} from '../domain/dateTime';
import { adifModeForLoggerMode } from '../domain/modes';
import { validateExchangeField } from '../domain/validation';
import { useNotifications } from '../lib/notificationsContext';
import { sanitizeLogCellUpdate } from './logWindowHelpers';

const COLUMN_PADDING_CHARS = 2;
const FIXED_COLUMN_WIDTHS = {
  'date-time': 19,
  frequency: 7,
  mode: 3,
  call: 12,
  multipliers: 2,
  points: 2,
  operator: 12,
};
const VIRTUAL_ROW_HEIGHT_PX = 22;
const VIRTUAL_OVERSCAN_ROWS = 8;
const LOAD_MORE_THRESHOLD_PX = 120;

function entryMeta(entry) {
  return entry?.meta ?? {};
}

function entryAdif(entry) {
  return entry?.adif ?? entry ?? {};
}

function formatDetailValue(key, value) {
  if (
    typeof value === 'number' &&
    /(?:QSO_DATE_TIME_ON|TIME_ON|timeOnEpoch|timestamp|received_at)/i.test(key)
  ) {
    const milliseconds = value < 100000000000 ? value * 1000 : value;
    const date = new Date(milliseconds);
    if (!Number.isNaN(date.getTime())) {
      return date.toISOString().replace('T', ' ').replace('Z', ' UTC');
    }
  }
  if (value && typeof value === 'object') return JSON.stringify(value);
  return String(value ?? '');
}

function detailRowsForContact(entry) {
  return ['meta', 'adif']
    .flatMap((section, sectionIndex) =>
      Object.entries(section === 'meta' ? entryMeta(entry) : entryAdif(entry))
        .map(([key, value]) => ({
          key,
          sectionIndex,
          value: formatDetailValue(`${section}.${key}`, value),
        }))
        .filter(({ value }) => value.trim() !== ''),
    )
    .sort(
      (left, right) =>
        left.key.localeCompare(right.key) ||
        left.sectionIndex - right.sectionIndex,
    )
    .map(({ key, value }) => [key, value]);
}

function qsoEpoch(entry) {
  const adif = entryAdif(entry);
  const meta = entryMeta(entry);
  if (typeof adif.QSO_DATE_TIME_ON === 'number') {
    return adif.QSO_DATE_TIME_ON;
  }

  if (typeof meta.timeOnEpoch === 'number') {
    return meta.timeOnEpoch;
  }

  if (typeof adif.Time === 'number') {
    return adif.Time;
  }

  return epochFromLegacyQsoDateTime(entry);
}

function formatDateTime(entry) {
  const epoch = qsoEpoch(entry);
  if (epoch === null) {
    return '';
  }
  return formatUtcDateTime(epoch);
}

function formatFrequency(entry, field = 'FREQ') {
  const adif = entryAdif(entry);
  const frequency = adif[field] ?? adif.FREQ ?? adif.Freq;
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

function columnWidthChars(settings, column, radioMode) {
  const headerWidth = String(column.label).length;
  let dataWidth = FIXED_COLUMN_WIDTHS[column.id];

  const exchangeField = exchangeFieldForColumn(settings, column);
  if (!dataWidth && exchangeField) {
    dataWidth = parseFieldInput(exchangeField.input, radioMode).maxLength;
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

function exchangeValueForColumn(settings, column, entry) {
  const exchangeField = exchangeFieldForColumn(settings, column);
  if (!exchangeField) return null;
  const adif = entryAdif(entry);
  return adif[column.field] ?? entry[column.field] ?? '';
}

function contactMode(entry, fallbackMode = 'CW') {
  const adif = entryAdif(entry);
  return String(adif.MODE ?? adif.Mode ?? fallbackMode).toUpperCase();
}

function cellValidation(settings, column, entry, radioMode) {
  const exchangeField = exchangeFieldForColumn(settings, column);
  if (!exchangeField) {
    return { ok: true, error: '' };
  }
  return validateExchangeField(
    exchangeField,
    exchangeValueForColumn(settings, column, entry),
    contactMode(entry, radioMode),
    entryAdif(entry),
  );
}

function formatCell(column, entry) {
  if (column.format === 'date_time_utc') {
    return formatDateTime(entry);
  }

  if (column.format === 'frequency_khz') {
    return formatFrequency(entry, column.field);
  }

  const values = column.source === 'meta' ? entryMeta(entry) : entryAdif(entry);
  return values[column.field] ?? entry[column.field] ?? '';
}

function contactKey(entry, index) {
  const meta = entryMeta(entry);
  const adif = entryAdif(entry);
  if (meta.clientId) return `client:${meta.clientId}`;
  if (meta.id !== undefined && meta.id !== null) return `id:${meta.id}`;

  return `row:${adif.QSO_DATE_TIME_ON ?? adif.TIME_ON ?? adif.Time ?? 'row'}-${adif.CALL ?? adif.Call ?? index}`;
}

function contactRowClassName(entry, isSelected) {
  const classes = [];
  if (entryMeta(entry).status === 'Failed') classes.push('failed-contact');
  else if (entryMeta(entry).status !== 'Committed')
    classes.push('uncommitted-contact');
  if (isSelected) classes.push('selected-contact');
  return classes.join(' ') || undefined;
}

function contactRowTitle(entry) {
  const meta = entryMeta(entry);
  if (meta.status !== 'Failed') return undefined;
  return meta.error
    ? `Contact upload failed: ${meta.error}`
    : 'Contact upload failed.';
}

function editableFieldForColumn(column) {
  return column.editable ? column.field : null;
}

function parseDateTimeUtc(value) {
  return parseUtcDateTime(value);
}

function exchangeFieldForColumn(settings, column) {
  return (settings?.exchange ?? []).find(
    (field) => field.adif === column.field,
  );
}

function sanitizeUpdateInput(settings, column, value, radioMode) {
  return sanitizeLogCellUpdate(settings?.exchange, column, value, radioMode);
}

function parseUpdateValue(settings, column, value, radioMode, entry = null) {
  if (column.format === 'date_time_utc') {
    const epoch = parseDateTimeUtc(value);
    if (epoch === null) {
      return {
        ok: false,
        error: 'Enter date/time as YYYY-MM-DD HH:MM:SS in UTC.',
      };
    }

    return { ok: true, value: epoch };
  }

  if (column.format === 'frequency_khz') {
    const parsedFrequency = Number.parseFloat(String(value));
    if (!Number.isFinite(parsedFrequency) || parsedFrequency <= 0) {
      return { ok: false, error: 'Enter a valid frequency.' };
    }

    return {
      ok: true,
      value: Math.round(parsedFrequency * 1000),
    };
  }

  if (column.field === 'MODE') {
    const mode = adifModeForLoggerMode(value);
    if (
      (settings?.modes ?? []).length > 0 &&
      !settings.modes.some(
        (allowedMode) => String(allowedMode).trim().toUpperCase() === mode,
      )
    ) {
      return {
        ok: false,
        error: `Enter one of: ${settings.modes.join(', ')}.`,
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
  if (exchangeField) {
    const validation = validateExchangeField(
      exchangeField,
      sanitizedValue,
      validationMode,
      entryAdif(entry),
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
  const columns = settings?.qso_table?.columns ?? [];
  const [selectedKeys, setSelectedKeys] = useState(() => new Set());
  const [contextMenu, setContextMenu] = useState(null);
  const [detailsContact, setDetailsContact] = useState(null);
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
    const field = editableFieldForColumn(contextMenu.column);
    if (!field) return;
    const contactIndex = contacts.findIndex(
      (entry, index) => contactKey(entry, index) === contextMenu.contactKey,
    );
    if (contactIndex === -1) return;

    setEditingCell({
      key: contextMenu.contactKey,
      column: contextMenu.column,
      value: String(formatCell(contextMenu.column, contacts[contactIndex])),
    });
    setContextMenu(null);
  }

  function viewDetails() {
    if (!contextMenu) return;
    const contact = contacts.find(
      (entry, index) => contactKey(entry, index) === contextMenu.contactKey,
    );
    setContextMenu(null);
    if (contact) setDetailsContact(contact);
  }

  function deleteSelected() {
    const contactsToDelete = selectedContacts();
    setContextMenu(null);
    onDeleteContacts?.(contactsToDelete);
  }

  function finishUpdate() {
    if (!editingCell) return;
    const field = editableFieldForColumn(editingCell.column);
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
        dedupeKey: `LogWindow.inlineEdit:${editingCell.column.id}:${parsed.error}`,
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
          {settings?.id ?? 'Loading contest...'}
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
                key={column.id}
                style={columnWidthStyle(settings, column, radioMode, columns)}
              />
            ))}
          </colgroup>
          <thead>
            <tr>
              {columns.map((column) => (
                <th key={column.id}>{column.label}</th>
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
                          editingCell.column.id === column.id;
                        const validation = cellValidation(
                          settings,
                          column,
                          entry,
                          radioMode,
                        );
                        return (
                          <td
                            key={column.id}
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
                                      contactMode(entry, radioMode),
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
                              formatCell(column, entry)
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
              <button type="button" onClick={viewDetails}>
                View details…
              </button>
              <button
                type="button"
                disabled={!editableFieldForColumn(contextMenu.column)}
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
      {detailsContact
        ? createPortal(
            <div
              className="log-details-dialog-overlay"
              onClick={() => setDetailsContact(null)}
            >
              <div
                role="dialog"
                aria-modal="true"
                aria-label="QSO details"
                className="log-details-dialog"
                onClick={(event) => event.stopPropagation()}
              >
                <div className="log-details-dialog-header">
                  <strong>QSO Details</strong>
                  <button
                    className="title-button"
                    type="button"
                    aria-label="Close QSO details"
                    onClick={() => setDetailsContact(null)}
                  >
                    ×
                  </button>
                </div>
                <div className="log-details-dialog-body">
                  <table className="band-map-details-table">
                    <tbody>
                      {detailRowsForContact(detailsContact).map(
                        ([label, value]) => (
                          <tr key={label}>
                            <th>{label}</th>
                            <td>{value}</td>
                          </tr>
                        ),
                      )}
                    </tbody>
                  </table>
                </div>
              </div>
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}

export default LogWindow;
