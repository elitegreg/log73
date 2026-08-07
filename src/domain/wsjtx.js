export const DEFAULT_WSJTX_PORT = 2237;

export function nextAvailableWsjtxPort(radios, start = DEFAULT_WSJTX_PORT) {
  const reserved = new Set(
    (Array.isArray(radios) ? radios : [])
      .filter((radio) => Boolean(radio?.wsjtx_enabled))
      .map((radio) => Number(radio?.wsjtx_port))
      .filter((port) => Number.isInteger(port)),
  );
  for (
    let port = Math.max(1024, Number(start) || DEFAULT_WSJTX_PORT);
    port <= 65535;
    port += 1
  ) {
    if (!reserved.has(port)) return port;
  }
  return DEFAULT_WSJTX_PORT;
}

export function wsjtxDataModeLocked(radio, mode) {
  return wsjtxTargetControlVisible(Boolean(radio?.wsjtx_enabled), mode);
}

export function wsjtxTargetControlVisible(wsjtxEnabled, mode) {
  return (
    Boolean(wsjtxEnabled) &&
    String(mode ?? '')
      .trim()
      .toUpperCase() === 'DATA'
  );
}
