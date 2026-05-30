import React from 'react';
import { messageButtonLabel } from '../mainWindowHelpers';

function FunctionKeys({
  activeMessageLabels,
  activeMessageKeys,
  sendMessageKey,
  stationCallsign,
  messageModeKey,
  repeatRunF1,
  setRepeatRunF1,
  esmNextKeys = [],
}) {
  const esmNextKeySet = new Set(esmNextKeys);

  return (
    <div className="function-keys">
      <div className="f-row">
        {activeMessageLabels.slice(0, 6).map((button) => (
          <button
            key={button.key}
            className={`f-key ${activeMessageKeys.has(button.key) ? 'active' : ''} ${esmNextKeySet.has(button.key) ? 'esm-next' : ''}`.trim()}
            type="button"
            title={`Keyboard shortcut: ${button.key}`}
            onClick={() => sendMessageKey(button.key)}
          >
            {button.key} {messageButtonLabel(button.label, stationCallsign)}
            {messageModeKey === 'run' && button.key === 'F1' && (
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
        {activeMessageLabels.slice(6, 12).map((button) => (
          <button
            key={button.key}
            className={`f-key ${activeMessageKeys.has(button.key) ? 'active' : ''} ${esmNextKeySet.has(button.key) ? 'esm-next' : ''}`.trim()}
            type="button"
            title={`Keyboard shortcut: ${button.key}`}
            onClick={() => sendMessageKey(button.key)}
          >
            {button.key} {messageButtonLabel(button.label, stationCallsign)}
          </button>
        ))}
      </div>
    </div>
  );
}

export default FunctionKeys;
