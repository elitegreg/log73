import React from 'react';

function CommandButtons({ stopCwSending, resetEntryFields, logContact, onRescore, handleQrzClick }) {
  return (
    <div className="command-buttons">
      <button className="cmd-btn" type="button" title="Keyboard shortcut: Esc" onClick={stopCwSending}>Stop Sending</button>
      <button className="cmd-btn" onClick={resetEntryFields}>Wipe</button>
      <button className="cmd-btn" onClick={() => logContact(false)}>Log it</button>
      <button className="cmd-btn" type="button" onClick={onRescore}>Rescore</button>
      <button className="cmd-btn">Mark</button>
      <button className="cmd-btn">Store</button>
      <button className="cmd-btn">Spot It</button>
      <button className="cmd-btn" type="button" onClick={handleQrzClick}>QRZ</button>
    </div>
  );
}

export default CommandButtons;
