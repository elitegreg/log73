const EXPORT_SETTINGS_STORAGE_PREFIX = 'cabrilloExportSettings:';

export function exportSettingsStorageKey(contestId) {
  return `${EXPORT_SETTINGS_STORAGE_PREFIX}${contestId ?? ''}`;
}

function fieldFallbackValue(fieldName, contestParams) {
  if (fieldName !== 'LOCATION') {
    return '';
  }

  return (
    contestParams.Location ??
    contestParams.State ??
    contestParams.County ??
    ''
  );
}

export function defaultExportValues(settings, log, storedValues = {}) {
  const contestParams = log?.contest_params ?? {};

  return Object.fromEntries(
    (settings?.cabrillo?.export_fields ?? []).map((field) => {
      let value = '';
      if (Object.hasOwn(storedValues, field.name)) {
        value = storedValues[field.name] ?? '';
      } else if (Object.hasOwn(contestParams, field.name)) {
        value = contestParams[field.name] ?? '';
      } else if (field.default !== undefined && field.default !== null) {
        value = field.default;
      } else {
        value = fieldFallbackValue(field.name, contestParams);
      }
      return [field.name, String(value)];
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
