import React, { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';
import { THEME_OPTIONS, ZOOM_OPTIONS } from '../themes/themes';

function ConfigScreen({ theme, onSetTheme, zoom, onSetZoom }) {
  const navigate = useNavigate();
  const { notifyError } = useNotifications();
  const [loginUser, setLoginUser] = useState('');
  const [loginPassword, setLoginPassword] = useState('');
  const [loginPasswordConfirm, setLoginPasswordConfirm] = useState('');
  const [loginEnabled, setLoginEnabled] = useState(false);
  const [dxClusterEnabled, setDxClusterEnabled] = useState(false);
  const [dxClusterHost, setDxClusterHost] = useState('');
  const [dxClusterPort, setDxClusterPort] = useState('23');
  const [dxClusterCallsign, setDxClusterCallsign] = useState('');
  const [dxClusterMaxAgeMin, setDxClusterMaxAgeMin] = useState('60');
  const [dxClusterCommands, setDxClusterCommands] = useState('');

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

  useEffect(() => {
    apiJson('/config')
      .then((result) => {
        if (!result.ok)
          throw new Error(result.error ?? 'Unable to load config');
        setLoginUser(result.config.login_user ?? '');
        setLoginEnabled(Boolean(result.config.login_enabled));
        setDxClusterEnabled(Boolean(result.config.dxcluster_enabled));
        setDxClusterHost(result.config.dxcluster_host ?? '');
        setDxClusterPort(String(result.config.dxcluster_port ?? 23));
        setDxClusterCallsign(result.config.dxcluster_callsign ?? '');
        setDxClusterMaxAgeMin(
          String(result.config.dxcluster_max_age_min ?? 60),
        );
        setDxClusterCommands(result.config.dxcluster_commands ?? '');
      })
      .catch((error) =>
        notifyOperationalError(
          'ConfigScreen.loadConfig',
          'Unable to load config.',
          error,
        ),
      );
  }, [notifyOperationalError]);

  async function saveConfig(event) {
    event.preventDefault();
    if (loginPassword !== loginPasswordConfirm) {
      notifyError('Passwords do not match.', {
        dedupeKey: 'ConfigScreen.passwordMismatch',
      });
      return;
    }

    const result = await apiJson('/config', {
      method: 'PUT',
      body: JSON.stringify({
        login_user: loginUser,
        login_password: loginPassword,
        login_password_confirm: loginPasswordConfirm,
        dxcluster_enabled: dxClusterEnabled,
        dxcluster_host: dxClusterHost,
        dxcluster_port: Number.parseInt(dxClusterPort, 10) || 23,
        dxcluster_callsign: dxClusterCallsign,
        dxcluster_max_age_min: Number.parseInt(dxClusterMaxAgeMin, 10) || 60,
        dxcluster_commands: dxClusterCommands,
      }),
    });

    if (!result.ok) {
      notifyOperationalError(
        'ConfigScreen.saveConfig',
        'Unable to save config.',
        result.error,
      );
      return;
    }

    navigate('/ui/open_log');
  }

  return (
    <form className="form-window" onSubmit={saveConfig}>
      <div className="title-bar">Log73 - Configure</div>
      <div
        className="selection-actions"
        style={{ justifyContent: 'space-between', padding: '8px 12px 0' }}
      >
        <label className="theme-selector">
          Theme:
          <select
            value={theme}
            onChange={(event) => onSetTheme?.(event.target.value)}
          >
            {THEME_OPTIONS.map((themeOption) => (
              <option key={themeOption.id} value={themeOption.id}>
                {themeOption.label}
              </option>
            ))}
          </select>
        </label>
        <label className="theme-selector">
          Zoom:
          <select
            value={String(zoom)}
            onChange={(event) => onSetZoom?.(Number(event.target.value))}
          >
            {ZOOM_OPTIONS.map((zoomOption) => (
              <option key={zoomOption.value} value={String(zoomOption.value)}>
                {zoomOption.label}
              </option>
            ))}
          </select>
        </label>
        <span>{loginEnabled ? 'Login is enabled.' : 'Login is disabled.'}</span>
      </div>
      <label>
        Username
        <input
          value={loginUser}
          onChange={(event) => setLoginUser(event.target.value)}
        />
      </label>
      <label>
        Password
        <input
          type="password"
          value={loginPassword}
          onChange={(event) => setLoginPassword(event.target.value)}
        />
      </label>
      <label>
        Confirm Password
        <input
          type="password"
          value={loginPasswordConfirm}
          onChange={(event) => setLoginPasswordConfirm(event.target.value)}
        />
      </label>
      <div
        className="selection-actions"
        style={{ justifyContent: 'flex-start', padding: '0 12px' }}
      >
        <span>Leave both password fields blank to disable login.</span>
      </div>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={dxClusterEnabled}
          onChange={(event) => setDxClusterEnabled(event.target.checked)}
        />
        Enable DxCluster
      </label>
      <label>
        DxCluster Host
        <input
          value={dxClusterHost}
          onChange={(event) => setDxClusterHost(event.target.value)}
        />
      </label>
      <label>
        DxCluster Port
        <input
          type="number"
          min="0"
          max="65535"
          value={dxClusterPort}
          onChange={(event) => setDxClusterPort(event.target.value)}
        />
      </label>
      <label>
        DxCluster Callsign
        <input
          value={dxClusterCallsign}
          onChange={(event) =>
            setDxClusterCallsign(event.target.value.toUpperCase())
          }
        />
      </label>
      <label>
        DxCluster Max Age (minutes)
        <input
          type="number"
          min="15"
          max="360"
          value={dxClusterMaxAgeMin}
          onChange={(event) => setDxClusterMaxAgeMin(event.target.value)}
        />
      </label>
      <label>
        DxCluster Commands
        <textarea
          value={dxClusterCommands}
          onChange={(event) => setDxClusterCommands(event.target.value)}
          placeholder="One optional command per line"
        />
      </label>
      <div className="selection-actions">
        <button className="cmd-btn primary" type="submit">
          Save
        </button>
        <button
          className="cmd-btn"
          type="button"
          onClick={() => navigate('/ui/open_log')}
        >
          Cancel
        </button>
      </div>
    </form>
  );
}

export default ConfigScreen;
