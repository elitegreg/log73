import React, { useEffect, useRef, useState } from 'react';
import { validateCallsign } from '../domain/validation';

function OperatorCallsignPrompt({ callsign, onAccept }) {
  const [value, setValue] = useState(callsign);
  const inputRef = useRef(null);
  const validation = validateCallsign(value);

  useEffect(() => {
    setValue(callsign);
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [callsign]);

  return (
    <div className="logger-prompt-dialog-overlay">
      <form
        className="logger-prompt-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="operator-callsign-prompt-label"
        onSubmit={(event) => {
          event.preventDefault();
          if (!validation.ok) {
            inputRef.current?.focus();
            return;
          }
          onAccept(value);
        }}
        onKeyDown={(event) => {
          event.stopPropagation();
          if (event.key === 'Escape') event.preventDefault();
        }}
      >
        <label
          className="logger-prompt-dialog-question"
          id="operator-callsign-prompt-label"
          htmlFor="operator-callsign-prompt-input"
        >
          Operator Callsign
        </label>
        <input
          className={`operator-callsign-prompt-input${validation.ok ? '' : ' invalid-field'}`}
          id="operator-callsign-prompt-input"
          ref={inputRef}
          type="text"
          value={value}
          aria-describedby={
            validation.ok ? undefined : 'operator-callsign-prompt-error'
          }
          aria-invalid={validation.ok ? undefined : true}
          autoCapitalize="characters"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setValue(event.target.value)}
        />
        {!validation.ok ? (
          <div
            className="operator-callsign-prompt-error"
            id="operator-callsign-prompt-error"
            role="alert"
          >
            {validation.error}
          </div>
        ) : null}
        <div className="logger-prompt-dialog-actions">
          <button
            className="cmd-btn primary"
            type="submit"
            disabled={!validation.ok}
          >
            OK
          </button>
        </div>
      </form>
    </div>
  );
}

export default OperatorCallsignPrompt;
