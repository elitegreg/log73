import React from 'react';
import { parseFieldInput } from '../../domain/contactFields';
import {
  validateCallsign,
  validateExchangeField,
} from '../../domain/validation';
import { CALLSIGN_FIELD_WIDTH_CHARS } from '../mainWindowHelpers';

function EntryFields({
  settings,
  radioMode,
  conditionFields,
  callSignRef,
  callSign,
  dxccLabel,
  dupeAlertText,
  handleCallsignChange,
  handleCallsignKeyDown,
  setActiveCompletionField,
  exchangeValue,
  exchangeInputRefs,
  updateExchangeField,
  handleExchangeKeyDown,
  locked = false,
}) {
  const callsignValidation = validateCallsign(callSign);

  return (
    <div className="entry-fields">
      <label
        className="entry-field"
        style={{
          flex: `${CALLSIGN_FIELD_WIDTH_CHARS} 1 ${CALLSIGN_FIELD_WIDTH_CHARS}em`,
        }}
      >
        <span className="dupe-alert" aria-live="polite">
          {dupeAlertText}
        </span>
        <span>
          Callsign
          {dxccLabel ? (
            <span className="callsign-dxcc-hint">{dxccLabel}</span>
          ) : null}
        </span>
        <input
          ref={callSignRef}
          type="text"
          value={callSign}
          onChange={handleCallsignChange}
          onKeyDown={handleCallsignKeyDown}
          onFocus={() => setActiveCompletionField('CALL')}
          onBlur={() => setActiveCompletionField(null)}
          className={`callsign${locked || callsignValidation.ok ? '' : ' invalid-field'}`}
          title={
            locked || callsignValidation.ok
              ? undefined
              : callsignValidation.error
          }
          aria-invalid={locked || callsignValidation.ok ? undefined : true}
          readOnly={locked}
          tabIndex={locked ? -1 : undefined}
          maxLength={12}
        />
      </label>
      {settings?.exchange?.map((field, index) => {
        const { kind, maxLength } = parseFieldInput(field.input, radioMode);
        const value = exchangeValue(field);
        const validation = validateExchangeField(
          field,
          value,
          radioMode,
          conditionFields,
        );
        const fieldWidthChars = Math.max(maxLength + 1, field.label.length, 4);
        const readOnly =
          locked ||
          field.fixed === true ||
          (field.direction === 'sent' && kind === 'SERIAL');

        return (
          <label
            className="entry-field"
            key={field.id}
            style={{ flex: `${fieldWidthChars} 1 ${fieldWidthChars}em` }}
          >
            <span>{field.label}</span>
            <input
              ref={(element) => {
                if (element) exchangeInputRefs.current[field.id] = element;
                else delete exchangeInputRefs.current[field.id];
              }}
              type="text"
              inputMode={
                kind === 'NUMERIC' || kind === 'SERIAL' || kind === 'RST'
                  ? 'numeric'
                  : 'text'
              }
              value={value}
              onChange={(event) =>
                updateExchangeField(field, event.target.value)
              }
              onKeyDown={(event) => handleExchangeKeyDown(event, index)}
              onFocus={() =>
                setActiveCompletionField(readOnly ? null : field.id)
              }
              onBlur={() => setActiveCompletionField(null)}
              readOnly={readOnly}
              tabIndex={readOnly ? -1 : undefined}
              className={`${readOnly ? 'fixed-field' : ''}${locked || validation.ok ? '' : ' invalid-field'}`.trim()}
              title={locked || validation.ok ? undefined : validation.error}
              aria-invalid={locked || validation.ok ? undefined : true}
              maxLength={maxLength}
            />
          </label>
        );
      })}
    </div>
  );
}

export default EntryFields;
