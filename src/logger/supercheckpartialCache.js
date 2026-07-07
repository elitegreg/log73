export function normalizeSupercheckpartialCallsign(value) {
  const normalized = String(value ?? '')
    .trim()
    .toUpperCase();
  return normalized || null;
}

export function mergeSupercheckpartialCallsigns(currentCallsigns, nextCallsigns) {
  const merged = [];
  const seen = new Set();

  for (const value of [...(currentCallsigns ?? []), ...(nextCallsigns ?? [])]) {
    const normalized = normalizeSupercheckpartialCallsign(value);
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    merged.push(normalized);
  }

  return merged;
}
