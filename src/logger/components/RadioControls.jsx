import React from 'react';
import { wsjtxTargetControlVisible } from '../../domain/wsjtx';
import {
  MODE_OPTIONS,
  catIndicatorState,
  isSelectableMode,
  modeIsCw,
} from '../mainWindowHelpers';

function RadioControls({
  operatingMode,
  setOperatingMode,
  currentBandAllowed,
  currentBandValue,
  bandOptions,
  currentBand,
  handleBandChange,
  radioMode,
  onSetRadioMode,
  esmEnabled,
  onSetEsmEnabled,
  wsjtxEnabled,
  wsjtxTarget,
  onSetWsjtXTarget,
  cwWpm,
  cwWpmMin,
  cwWpmMax,
  handleCwWpmChange,
  bandMapEnabled,
  onSetBandMapEnabled,
  backendSocketStatus,
  radioSocketStatus,
  catStatus,
  manualEntryDisabled = false,
}) {
  const modeSelectable = isSelectableMode(radioMode);
  const modeOptions = modeSelectable
    ? MODE_OPTIONS
    : [...MODE_OPTIONS, radioMode].filter(Boolean);
  const showWsjtXTarget = wsjtxTargetControlVisible(wsjtxEnabled, radioMode);
  const catIndicator = catIndicatorState(radioSocketStatus, catStatus);
  const catTitle =
    catIndicator === 'connected'
      ? 'CAT online'
      : catIndicator === 'degraded'
        ? 'CAT offline; radio control WebSocket is connected'
        : `CAT radio control WebSocket ${radioSocketStatus}`;

  return (
    <div className="radio-controls">
      <label className="radio-control">
        Run Mode:
        <select
          value={operatingMode}
          onChange={(event) => setOperatingMode(event.target.value)}
        >
          <option value="S&P">S&amp;P</option>
          <option value="Run">Run</option>
        </select>
      </label>
      <label
        className={
          currentBandAllowed ? 'radio-control' : 'radio-control unsupported'
        }
      >
        Band:
        <select value={currentBandValue} onChange={handleBandChange}>
          {bandOptions.map((band) => (
            <option key={band.name} value={band.name}>
              {band.name}
            </option>
          ))}
          {!currentBand && <option value="unknown">Unknown</option>}
        </select>
      </label>
      <label
        className={
          modeSelectable ? 'radio-control' : 'radio-control unsupported'
        }
      >
        Mode:
        <select
          value={radioMode}
          onChange={(event) => onSetRadioMode?.(event.target.value)}
        >
          {modeOptions.map((mode) => (
            <option key={mode} value={mode}>
              {mode}
            </option>
          ))}
        </select>
      </label>
      {showWsjtXTarget ? (
        <label className="radio-control esm-toggle">
          WSJT-X Target:
          <input
            type="checkbox"
            checked={Boolean(wsjtxTarget)}
            onChange={(event) => onSetWsjtXTarget?.(event.target.checked)}
            disabled={radioSocketStatus !== 'connected'}
          />
        </label>
      ) : (
        <label className="radio-control esm-toggle">
          ESM:
          <input
            type="checkbox"
            checked={esmEnabled}
            onChange={(event) => onSetEsmEnabled?.(event.target.checked)}
            disabled={manualEntryDisabled}
          />
        </label>
      )}
      {modeIsCw(radioMode) && (
        <label className="radio-control cw-wpm-control">
          CW WPM:
          <input
            type="number"
            min={cwWpmMin}
            max={cwWpmMax}
            step="1"
            value={cwWpm}
            onChange={handleCwWpmChange}
            disabled={radioSocketStatus !== 'connected'}
          />
        </label>
      )}
      <label className="radio-control band-map-toggle">
        Band Map:
        <input
          type="checkbox"
          checked={bandMapEnabled}
          onChange={(event) => onSetBandMapEnabled?.(event.target.checked)}
        />
      </label>
      <div className="backend-status-group">
        <div className="backend-socket-status" title={catTitle}>
          <span
            className={`backend-socket-light ${catIndicator}`}
            aria-hidden="true"
          />
          CAT
        </div>
        <div
          className="backend-socket-status"
          title={`Server ${backendSocketStatus}`}
        >
          <span
            className={`backend-socket-light ${backendSocketStatus === 'connected' ? 'connected' : 'disconnected'}`}
            aria-hidden="true"
          />
          Server
        </div>
      </div>
    </div>
  );
}

export default RadioControls;
