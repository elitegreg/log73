import React, { useEffect, useRef } from 'react';

function TransmitterIdPrompt({ prompt, onSelect }) {
  const firstButtonRef = useRef(null);

  useEffect(() => {
    firstButtonRef.current?.focus();
  }, [prompt]);

  return (
    <div className="transmitter-id-dialog-overlay">
      <div
        className="transmitter-id-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Transmitter selection"
      >
        {prompt.question ? (
          <div className="transmitter-id-dialog-question">
            {prompt.question}
          </div>
        ) : null}
        <div className="transmitter-id-dialog-actions">
          {prompt.options.map((option, index) => (
            <button
              className="cmd-btn primary"
              key={option.id}
              ref={index === 0 ? firstButtonRef : null}
              type="button"
              onClick={() => onSelect(option.id)}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

export default TransmitterIdPrompt;
