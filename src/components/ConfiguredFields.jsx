import React from 'react';
import { parseFieldInput } from '../domain/contactFields';

function ConfiguredFields({
  fields,
  values,
  onChange,
  radioMode = 'CW',
  disabled = false,
}) {
  return fields.map((field) => {
    const { kind, maxLength } = parseFieldInput(field.input, radioMode);
    const widget = String(field.widget ?? '').toLowerCase();
    const validValues = field.validation?.values ?? [];
    const value = values[field.key] ?? '';
    const commonProps = {
      value,
      onChange: (event) => onChange(field, event.target.value),
      required: field.validation?.required !== false,
      disabled,
    };

    let input = null;
    if (widget === 'select' && validValues.length > 0) {
      input = (
        <select {...commonProps}>
          <option value="">Select...</option>
          {validValues.map((validValue) => (
            <option key={validValue} value={validValue}>
              {validValue}
            </option>
          ))}
        </select>
      );
    } else if (widget === 'textarea') {
      input = (
        <textarea
          {...commonProps}
          rows={Math.max(3, Math.min(field.max_lines ?? 4, 8))}
        />
      );
    } else {
      input = (
        <input
          {...commonProps}
          pattern={field.validation?.pattern ?? undefined}
          inputMode={
            kind === 'NUMERIC' || kind === 'SERIAL' ? 'numeric' : 'text'
          }
          maxLength={maxLength}
          autoCapitalize={field.preserve_case === true ? 'off' : 'characters'}
          spellCheck={field.preserve_case === true}
        />
      );
    }

    return (
      <label key={field.id}>
        {field.label ?? field.key}
        {input}
        {field.help_text ? (
          <span className="field-help">{field.help_text}</span>
        ) : null}
      </label>
    );
  });
}

export default ConfiguredFields;
