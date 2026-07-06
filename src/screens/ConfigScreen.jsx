import React, { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { apiJson } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';
import { buildConfigUpdatePayload } from '../domain/configUpdate';
import {
  DEFAULT_LOGGER_IMAGE_URL,
  loadLoggerImageUrl,
  saveLoggerImageUrl,
} from '../domain/loggerImageSettings';
import { THEME_OPTIONS, ZOOM_OPTIONS } from '../themes/themes';

function ConfigScreen({ theme, onSetTheme, zoom, onSetZoom }) {
  const navigate = useNavigate();
  const { notifyError } = useNotifications();
  const [loginUser, setLoginUser] = useState('');
  const [loginPassword, setLoginPassword] = useState('');
  const [loginPasswordConfirm, setLoginPasswordConfirm] = useState('');
  const [loginEnabled, setLoginEnabled] = useState(false);
  const [disableLogin, setDisableLogin] = useState(false);
  const [dxClusterEnabled, setDxClusterEnabled] = useState(false);
  const [dxClusterHost, setDxClusterHost] = useState('');
  const [dxClusterPort, setDxClusterPort] = useState('23');
  const [dxClusterCallsign, setDxClusterCallsign] = useState('');
  const [dxClusterMaxAgeMin, setDxClusterMaxAgeMin] = useState('60');
  const [dxClusterCommands, setDxClusterCommands] = useState('');
  const [loggerImageUrl, setLoggerImageUrl] = useState(loadLoggerImageUrl);

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
      .then((config) => {
        setLoginUser(config.login_user ?? '');
        setLoginEnabled(Boolean(config.login_enabled));
        setDisableLogin(false);
        setDxClusterEnabled(Boolean(config.dxcluster_enabled));
        setDxClusterHost(config.dxcluster_host ?? '');
        setDxClusterPort(String(config.dxcluster_port ?? 23));
        setDxClusterCallsign(config.dxcluster_callsign ?? '');
        setDxClusterMaxAgeMin(String(config.dxcluster_max_age_min ?? 60));
        setDxClusterCommands(config.dxcluster_commands ?? '');
      })
      .catch((error) =>
        notifyOperationalError(
          'ConfigScreen.loadConfig',
          'Unable to load config.',
          error,
        ),
      );
  }, [notifyOperationalError]);

  function resetToDefaults() {
    setLoginUser('');
    setLoginPassword('');
    setLoginPasswordConfirm('');
    setLoginEnabled(false);
    setDisableLogin(true);
    setDxClusterEnabled(false);
    setDxClusterHost('');
    setDxClusterPort('23');
    setDxClusterCallsign('');
    setDxClusterMaxAgeMin('60');
    setDxClusterCommands('');
    setLoggerImageUrl(DEFAULT_LOGGER_IMAGE_URL);
    onSetTheme?.('default');
    onSetZoom?.(1);
  }

  function openHelpWindow() {
    window.open('/help/index.html', '_blank', 'noopener,noreferrer');
  }

  async function saveConfig(event) {
    event.preventDefault();
    const passwordChangeRequested =
      loginPassword !== '' || loginPasswordConfirm !== '';
    if (
      !disableLogin &&
      passwordChangeRequested &&
      loginPassword !== loginPasswordConfirm
    ) {
      notifyError('Passwords do not match.', {
        dedupeKey: 'ConfigScreen.passwordMismatch',
      });
      return;
    }

    try {
      await apiJson('/config', {
        method: 'PUT',
        body: JSON.stringify(
          buildConfigUpdatePayload({
            loginUser,
            loginPassword,
            loginPasswordConfirm,
            disableLogin,
            dxClusterEnabled,
            dxClusterHost,
            dxClusterPort,
            dxClusterCallsign,
            dxClusterMaxAgeMin,
            dxClusterCommands,
          }),
        ),
      });
    } catch (error) {
      notifyOperationalError(
        'ConfigScreen.saveConfig',
        'Unable to save config.',
        error,
      );
      return;
    }

    saveLoggerImageUrl(loggerImageUrl);
    navigate('/ui/open_log');
  }

  const loginStatus = disableLogin
    ? 'Login will be disabled on save.'
    : loginEnabled
      ? 'Login is enabled.'
      : 'Login is disabled.';

  return (
    <form className="form-window" onSubmit={saveConfig}>
      <div className="title-bar">
        <span>Log73 - Configure</span>
        <button
          className="title-button title-help-button"
          type="button"
          aria-label="Open help"
          title="Open help"
          onClick={openHelpWindow}
        >
          ?
        </button>
      </div>
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
        <span>{loginStatus}</span>
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
          disabled={disableLogin}
          onChange={(event) => setLoginPassword(event.target.value)}
        />
      </label>
      <label>
        Confirm Password
        <input
          type="password"
          value={loginPasswordConfirm}
          disabled={disableLogin}
          onChange={(event) => setLoginPasswordConfirm(event.target.value)}
        />
      </label>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={disableLogin}
          onChange={(event) => setDisableLogin(event.target.checked)}
        />
        Disable login on save
      </label>
      <div
        className="selection-actions"
        style={{ justifyContent: 'flex-start', padding: '0 12px' }}
      >
        <span>
          Leave password fields blank to keep the current password. Check
          Disable login to turn authentication off.
        </span>
      </div>
      <label>
        Logger side image URL
        <input
          value={loggerImageUrl}
          onChange={(event) => setLoggerImageUrl(event.target.value)}
          placeholder="https://www.hamqsl.com/solarn0nbh.php"
        />
      </label>
      <div
        className="selection-actions"
        style={{ justifyContent: 'flex-start', padding: '0 12px' }}
      >
        <span>Stored in this browser only.</span>
      </div>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={dxClusterEnabled}
          onChange={(event) => setDxClusterEnabled(event.target.checked)}
        />
        Enable DxCluster
      </label>
      {dxClusterEnabled ? (
        <>
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
        </>
      ) : null}
      <div className="selection-actions">
        <button className="cmd-btn" type="button" onClick={resetToDefaults}>
          Reset to Defaults
        </button>
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
