import React from 'react';
import { parseFieldType } from '../domain/contactFields';

function ConfiguredFields({
  fields,
  values,
  onChange,
  radioMode = 'CW',
  disabled = false,
}) {
  return fields.map((field) => {
    const { kind, maxLength } = parseFieldType(field.type, radioMode);
    const widget = String(field.widget ?? '').toLowerCase();
    const validValues = field.valid_values ?? [];
    const value = values[field.name] ?? '';
    const commonProps = {
      value,
      onChange: (event) => onChange(field, event.target.value),
      required: field.required !== false,
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
          pattern={field.regex ?? undefined}
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
      <label key={field.name}>
        {field.label ?? field.name}
        {input}
        {field.help_text ? (
          <span className="field-help">{field.help_text}</span>
        ) : null}
      </label>
    );
  });
}

export default ConfiguredFields;
