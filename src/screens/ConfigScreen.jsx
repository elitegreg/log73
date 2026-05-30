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
      <div className="title-bar">Log73 - Configure Login</div>
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
      <div
        className="selection-actions"
        style={{ justifyContent: 'flex-start', padding: '0 12px 10px' }}
      >
        <span>Leave both password fields blank to disable login.</span>
      </div>
    </form>
  );
}

export default ConfigScreen;
