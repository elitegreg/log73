import React from 'react';

function CommandButtons({
  stopMessageSending,
  clearEntryFields,
  logContact,
  onRescore,
  isRescoreLoading,
  disableRescore,
  handleQrzClick,
  handleMark,
  handleStore,
  handleSpotIt,
  highlightLogIt = false,
}) {
  return (
    <div className="command-buttons">
      <button
        className="cmd-btn"
        type="button"
        title="Keyboard shortcut: Esc"
        onClick={stopMessageSending}
      >
        Stop Sending
      </button>
      <button className="cmd-btn" onClick={clearEntryFields}>
        Clear
      </button>
      <button
        className={`cmd-btn${highlightLogIt ? ' esm-next' : ''}`}
        onClick={() => logContact(false)}
      >
        Log it
      </button>
      <button
        className="cmd-btn"
        type="button"
        onClick={onRescore}
        disabled={disableRescore}
      >
        {isRescoreLoading ? 'Rescoring...' : 'Rescore'}
      </button>
      <button
        className="cmd-btn"
        type="button"
        title="Keyboard shortcut: Alt-M"
        onClick={handleMark}
      >
        Mark
      </button>
      <button
        className="cmd-btn"
        type="button"
        title="Keyboard shortcut: Alt-O"
        onClick={handleStore}
      >
        Store
      </button>
      <button
        className="cmd-btn"
        type="button"
        title="Keyboard shortcut: Ctrl-P"
        onClick={handleSpotIt}
      >
        Spot It
      </button>
      <button className="cmd-btn" type="button" onClick={handleQrzClick}>
        QRZ
      </button>
    </div>
  );
}

export default CommandButtons;
