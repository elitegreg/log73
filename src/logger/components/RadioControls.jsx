import React from 'react';
import { MODE_OPTIONS } from '../mainWindowHelpers';

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
  cwWpm,
  cwWpmMin,
  cwWpmMax,
  handleCwWpmChange,
  backendSocketStatus,
  catStatus,
}) {
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
            <option key={band.meters} value={band.meters}>
              {band.name}
            </option>
          ))}
          {!currentBand && <option value="unknown">Unknown</option>}
        </select>
      </label>
      <label className="radio-control">
        Mode:
        <select
          value={radioMode}
          onChange={(event) => onSetRadioMode?.(event.target.value)}
        >
          {MODE_OPTIONS.map((mode) => (
            <option key={mode} value={mode}>
              {mode}
            </option>
          ))}
        </select>
      </label>
      {radioMode === 'CW' && (
        <label className="radio-control cw-wpm-control">
          CW WPM:
          <input
            type="number"
            min={cwWpmMin}
            max={cwWpmMax}
            step="1"
            value={cwWpm}
            onChange={handleCwWpmChange}
          />
        </label>
      )}
      <div className="backend-status-group">
        <div className="backend-socket-status" title={`CAT ${catStatus}`}>
          <span
            className={`backend-socket-light ${catStatus === 'online' ? 'connected' : 'disconnected'}`}
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
