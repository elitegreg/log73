export const NONE_SOUND_DEVICE_ID = '';

export function normalizeSoundDeviceId(value) {
  const normalized = String(value ?? '').trim();
  return normalized === '' ? null : normalized;
}

export function soundDeviceOptionLabel(device) {
  const name = String(
    device?.name ?? device?.description ?? device?.id ?? '',
  ).trim();
  const defaultText = device?.is_default ? ' (default)' : '';
  return `${name || 'Unknown sound device'}${defaultText}`;
}

export function soundDeviceHosts(devices) {
  return [
    ...new Set(
      (Array.isArray(devices) ? devices : [])
        .map((device) => String(device?.host ?? '').trim())
        .filter(Boolean),
    ),
  ].sort((left, right) => left.localeCompare(right));
}

export function filterSoundDevicesByHost(devices, host) {
  const normalizedHost = String(host ?? '').trim();
  if (!normalizedHost) return Array.isArray(devices) ? devices : [];
  return (Array.isArray(devices) ? devices : []).filter(
    (device) => String(device?.host ?? '').trim() === normalizedHost,
  );
}

export function preferredSoundDeviceHost(hosts, platform = '') {
  const normalizedHosts = Array.isArray(hosts) ? hosts : [];
  if (!/linux/i.test(platform)) return NONE_SOUND_DEVICE_ID;
  for (const preferredHost of ['pipewire', 'pulseaudio', 'alsa']) {
    const host = normalizedHosts.find(
      (candidate) => candidate.toLowerCase() === preferredHost,
    );
    if (host) return host;
  }
  return NONE_SOUND_DEVICE_ID;
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
