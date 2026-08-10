const EXPORT_SETTINGS_STORAGE_PREFIX = 'cabrilloExportSettings:';

export function exportSettingsStorageKey(contestId) {
  return `${EXPORT_SETTINGS_STORAGE_PREFIX}${contestId ?? ''}`;
}

function fieldFallbackValue(fieldName, contestParams) {
  if (fieldName !== 'LOCATION') {
    return '';
  }

  return (
    contestParams.Location ?? contestParams.State ?? contestParams.County ?? ''
  );
}

export function defaultExportValues(settings, log, storedValues = {}) {
  const contestParams = log?.contest_params ?? {};

  return Object.fromEntries(
    (settings?.cabrillo?.export_fields ?? []).map((field) => {
      let value = '';
      if (Object.hasOwn(storedValues, field.key)) {
        value = storedValues[field.key] ?? '';
      } else if (Object.hasOwn(contestParams, field.key)) {
        value = contestParams[field.key] ?? '';
      } else if (field.default !== undefined && field.default !== null) {
        value = field.default;
      } else {
        value = fieldFallbackValue(field.key, contestParams);
      }
      return [field.key, String(value)];
    }),
  );
}

export function loadStoredExportValues(contestId) {
  if (typeof localStorage === 'undefined' || !contestId) {
    return {};
  }

  try {
    const value = localStorage.getItem(exportSettingsStorageKey(contestId));
    if (!value) {
      return {};
    }
    const parsed = JSON.parse(value);
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return {};
    }
    return parsed;
  } catch {
    return {};
  }
}

export function saveStoredExportValues(contestId, values) {
  if (typeof localStorage === 'undefined' || !contestId) {
    return;
  }

  localStorage.setItem(
    exportSettingsStorageKey(contestId),
    JSON.stringify(values),
  );
}
