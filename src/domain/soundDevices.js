export const NONE_SOUND_DEVICE_ID = '';

export function normalizeSoundDeviceId(value) {
  const normalized = String(value ?? '').trim();
  return normalized === '' ? null : normalized;
}

export function soundDeviceOptionLabel(device) {
  const name = String(device?.name ?? device?.description ?? device?.id ?? '')
    .trim();
  const host = String(device?.host ?? '').trim();
  const defaultText = device?.is_default ? ' (default)' : '';
  const hostText = host ? ` [${host}]` : '';
  return `${name || 'Unknown sound device'}${hostText}${defaultText}`;
}

export function soundDeviceOptions(devices, selectedId = NONE_SOUND_DEVICE_ID) {
  const normalizedSelectedId = normalizeSoundDeviceId(selectedId);
  const seen = new Set();
  const options = [
    {
      id: NONE_SOUND_DEVICE_ID,
      label: 'None',
      device: null,
    },
  ];

  for (const device of Array.isArray(devices) ? devices : []) {
    const id = normalizeSoundDeviceId(device?.id);
    if (!id || seen.has(id)) continue;
    seen.add(id);
    options.push({
      id,
      label: soundDeviceOptionLabel(device),
      device,
    });
  }

  if (normalizedSelectedId && !seen.has(normalizedSelectedId)) {
    options.push({
      id: normalizedSelectedId,
      label: `${normalizedSelectedId} (not found)`,
      device: null,
    });
  }

  return options;
}
