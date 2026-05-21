export const MAX_COMPLETION_MATCHES = 100;

function normalizedQuery(value) {
  return String(value ?? '')
    .trim()
    .toUpperCase();
}

function matchingValues(
  values,
  query,
  minimumLength,
  maxMatches = MAX_COMPLETION_MATCHES,
) {
  const normalized = normalizedQuery(query);
  if (normalized.length < minimumLength) return [];

  return (values ?? [])
    .map((value) => String(value).toUpperCase())
    .filter((value) => value.includes(normalized))
    .slice(0, maxMatches);
}

export function callsignCompletionMatches(
  callsigns,
  query,
  maxMatches = MAX_COMPLETION_MATCHES,
) {
  return matchingValues(callsigns, query, 3, maxMatches);
}

export function exchangeCompletionMatches(
  field,
  value,
  maxMatches = MAX_COMPLETION_MATCHES,
) {
  return matchingValues(field?.valid_values ?? [], value, 1, maxMatches);
}
