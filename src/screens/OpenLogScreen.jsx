import React, { useCallback, useEffect, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { apiDownload, apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';

function OpenLogScreen() {
  const navigate = useNavigate();
  const { notifyError } = useNotifications();
  const [logs, setLogs] = useState([]);
  const [radios, setRadios] = useState([]);
  const [selectedLogId, setSelectedLogId] = useState('');
  const [selectedRadioId, setSelectedRadioId] = useState('');

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

  async function load() {
    const [nextLogs, nextRadios] = await Promise.all([
      apiJson('/logs'),
      apiJson('/radios'),
    ]);
    setLogs(nextLogs);
    setRadios(nextRadios);
    setSelectedLogId((current) => current || String(nextLogs[0]?.id ?? ''));
    setSelectedRadioId((current) => current || String(nextRadios[0]?.id ?? ''));
  }

  useEffect(() => {
    load().catch((error) =>
      notifyOperationalError(
        'OpenLogScreen.load',
        'Unable to load logs and radios.',
        error,
      ),
    );
  }, [notifyOperationalError]);

  async function deleteLog() {
    if (!selectedLogId) return;
    const result = await apiJson(`/logs/${selectedLogId}`, {
      method: 'DELETE',
    });
    if (!result.ok) {
      notifyOperationalError(
        'OpenLogScreen.deleteLog',
        'Unable to delete log.',
        result.error,
        { selectedLogId },
      );
      return;
    }
    setSelectedLogId('');
    await load().catch((error) =>
      notifyOperationalError(
        'OpenLogScreen.deleteLog.reload',
        'Unable to refresh logs after delete.',
        error,
        { selectedLogId },
      ),
    );
  }

  async function deleteRadio() {
    if (!selectedRadioId) return;
    const result = await apiJson(`/radios/${selectedRadioId}`, {
      method: 'DELETE',
    });
    if (!result.ok) {
      notifyOperationalError(
        'OpenLogScreen.deleteRadio',
        'Unable to delete radio.',
        result.error,
        { selectedRadioId },
      );
      return;
    }
    setSelectedRadioId('');
    await load().catch((error) =>
      notifyOperationalError(
        'OpenLogScreen.deleteRadio.reload',
        'Unable to refresh radios after delete.',
        error,
        { selectedRadioId },
      ),
    );
  }

  function openLogger() {
    if (!selectedLogId || !selectedRadioId) {
      notifyError('Select both a log and a radio before opening the logger.', {
        dedupeKey: 'OpenLogScreen.openLogger.selection',
      });
      return;
    }
    navigate(`/ui/logger/${selectedLogId}/${selectedRadioId}`);
  }

  async function exportAdif() {
    if (!selectedLogId) return;

    try {
      const { blob, filename } = await apiDownload(
        `/logs/${selectedLogId}/adif`,
        {
          method: 'POST',
        },
      );
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = filename;
      document.body.appendChild(link);
      link.click();
      link.remove();
      window.setTimeout(() => {
        URL.revokeObjectURL(url);
      }, 0);
    } catch (error) {
      notifyOperationalError(
        'OpenLogScreen.exportAdif',
        'Unable to export ADIF file.',
        error,
        { selectedLogId },
      );
    }
  }

  return (
    <div className="selection-window">
      <div className="title-bar">Log73 - Open Log</div>
      <div
        className="selection-actions"
        style={{ justifyContent: 'space-between', padding: '8px 12px 0' }}
      >
        <Link className="cmd-btn" to="/ui/config">
          Configure Log73
        </Link>
      </div>
      <div className="selection-grid">
        <section>
          <h2>Logs</h2>
          <select
            className="selection-list"
            size={10}
            value={selectedLogId}
            onChange={(event) => setSelectedLogId(event.target.value)}
          >
            {logs.map((log) => (
              <option key={log.id} value={log.id}>
                {log.name} - {log.station_callsign} - {log.contest_id}
              </option>
            ))}
          </select>
          <div className="selection-buttons">
            <Link className="cmd-btn" to="/ui/create_log">
              Create
            </Link>
            <Link
              className={`cmd-btn${selectedLogId ? '' : ' disabled'}`}
              to={selectedLogId ? `/ui/edit_log/${selectedLogId}` : '#'}
              onClick={(event) => {
                if (!selectedLogId) event.preventDefault();
              }}
            >
              Edit
            </Link>
            <button
              className="cmd-btn"
              onClick={deleteLog}
              disabled={!selectedLogId}
            >
              Delete
            </button>
            <Link
              className={`cmd-btn${selectedLogId ? '' : ' disabled'}`}
              to={selectedLogId ? `/ui/export_log/${selectedLogId}` : '#'}
              onClick={(event) => {
                if (!selectedLogId) event.preventDefault();
              }}
            >
              Export Cabrillo
            </Link>
            <button
              className="cmd-btn"
              onClick={exportAdif}
              disabled={!selectedLogId}
            >
              Export ADIF
            </button>
          </div>
        </section>
        <section>
          <h2>Radios</h2>
          <select
            className="selection-list"
            size={10}
            value={selectedRadioId}
            onChange={(event) => setSelectedRadioId(event.target.value)}
          >
            {radios.map((radio) => (
              <option key={radio.id} value={radio.id}>
                {radio.name} - {radio.rigctld_host}:{radio.rigctld_port} - poll{' '}
                {radio.poll_frequency}s timeout {radio.rigctld_timeout}s
              </option>
            ))}
          </select>
          <div className="selection-buttons">
            <Link className="cmd-btn" to="/ui/create_radio">
              Create
            </Link>
            <Link
              className={`cmd-btn${selectedRadioId ? '' : ' disabled'}`}
              to={selectedRadioId ? `/ui/edit_radio/${selectedRadioId}` : '#'}
              onClick={(event) => {
                if (!selectedRadioId) event.preventDefault();
              }}
            >
              Edit
            </Link>
            <button
              className="cmd-btn"
              onClick={deleteRadio}
              disabled={!selectedRadioId}
            >
              Delete
            </button>
          </div>
        </section>
      </div>
      <div className="selection-actions">
        <button className="cmd-btn primary" onClick={openLogger}>
          Open
        </button>
      </div>
    </div>
  );
}

export default OpenLogScreen;
