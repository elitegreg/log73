import React from 'react';
import { cwButtonLabel } from '../mainWindowHelpers';

function FunctionKeys({
  activeCwLabels,
  activeCwKeys,
  sendCwKey,
  stationCallsign,
  cwModeKey,
  repeatRunF1,
  setRepeatRunF1,
  esmNextKeys = [],
}) {
  const esmNextKeySet = new Set(esmNextKeys);

  return (
    <div className="function-keys">
      <div className="f-row">
        {activeCwLabels.slice(0, 6).map((button) => (
          <button
            key={button.key}
            className={`f-key ${activeCwKeys.has(button.key) ? 'active' : ''} ${esmNextKeySet.has(button.key) ? 'esm-next' : ''}`.trim()}
            type="button"
            title={`Keyboard shortcut: ${button.key}`}
            onClick={() => sendCwKey(button.key)}
          >
            {button.key} {cwButtonLabel(button.label, stationCallsign)}
            {cwModeKey === 'run' && button.key === 'F1' && (
              <label
                className="f-key-repeat"
                style={{ float: 'right' }}
                onClick={(event) => event.stopPropagation()}
              >
                <input
                  type="checkbox"
                  checked={repeatRunF1}
                  onChange={(event) => setRepeatRunF1(event.target.checked)}
                />
                Rpt
              </label>
            )}
          </button>
        ))}
      </div>
      <div className="f-row">
        {activeCwLabels.slice(6, 12).map((button) => (
          <button
            key={button.key}
            className={`f-key ${activeCwKeys.has(button.key) ? 'active' : ''} ${esmNextKeySet.has(button.key) ? 'esm-next' : ''}`.trim()}
            type="button"
            title={`Keyboard shortcut: ${button.key}`}
            onClick={() => sendCwKey(button.key)}
          >
            {button.key} {cwButtonLabel(button.label, stationCallsign)}
          </button>
        ))}
      </div>
    </div>
  );
}

export default FunctionKeys;
