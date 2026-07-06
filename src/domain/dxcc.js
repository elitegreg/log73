function normalizeCallsign(callsign) {
  return String(callsign ?? '')
    .trim()
    .toUpperCase();
}

export function splitCallsign(callsign) {
  const normalized = normalizeCallsign(callsign);
  if (!normalized) return null;

  const characters = [...normalized];
  const firstSearchIndex = /^\d$/.test(characters[0]) ? 1 : 0;
  let separatorStart = -1;
  let separatorEnd = -1;

  for (let index = firstSearchIndex; index < characters.length; index += 1) {
    if (!/^\d$/.test(characters[index])) continue;
    separatorStart = index;
    separatorEnd = index + 1;
    while (
      separatorEnd < characters.length &&
      /^\d$/.test(characters[separatorEnd])
    ) {
      separatorEnd += 1;
    }
    break;
  }

  if (separatorStart <= 0) return null;

  return {
    prefix: characters.slice(0, separatorStart).join(''),
    number: characters.slice(separatorStart, separatorEnd).join(''),
    suffix: characters.slice(separatorEnd).join(''),
  };
}

export function callsignPrefix(callsign) {
  return splitCallsign(callsign)?.prefix ?? null;
}

export function callsignFilterPrefix(callsign) {
  const parts = splitCallsign(callsign);
  return parts ? `${parts.prefix}${parts.number}` : '';
}

export function lookupDxcc(database, callsign) {
  const normalizedCallsign = normalizeCallsign(callsign);
  if (!normalizedCallsign || !callsignPrefix(normalizedCallsign)) return null;

  const rules = Array.isArray(database?.rules) ? database.rules : [];
  const entities = Array.isArray(database?.entities) ? database.entities : [];
  const exactMatch = rules.find(
    (rule) => rule?.exact === true && rule.pattern === normalizedCallsign,
  );
  if (exactMatch) return dxccInfoForRule(exactMatch, entities);

  let bestRule = null;
  for (const rule of rules) {
    if (rule?.exact === true) continue;
    if (!normalizedCallsign.startsWith(String(rule?.pattern ?? ''))) continue;
    if (
      !bestRule ||
      String(rule.pattern).length > String(bestRule.pattern).length
    ) {
      bestRule = rule;
    }
  }

  return bestRule ? dxccInfoForRule(bestRule, entities) : null;
}

export function dxccLabel(dxccInfo) {
  const countryName = String(dxccInfo?.country_name ?? '').trim();
  const continent = String(dxccInfo?.continent ?? '')
    .trim()
    .toUpperCase();
  if (!countryName || !continent) return '';
  return `${countryName} ${continent}`;
}

function dxccInfoForRule(rule, entities) {
  const entity = entities?.[rule?.entity_index];
  if (!entity) return null;

  return {
    country_name: entity.country_name,
    cq_zone: rule.cq_zone ?? entity.cq_zone,
    itu_zone: rule.itu_zone ?? entity.itu_zone,
    continent: rule.continent ?? entity.continent,
    latitude: rule.latitude ?? entity.latitude,
    longitude: rule.longitude ?? entity.longitude,
    utc_offset: rule.utc_offset ?? entity.utc_offset,
    primary_prefix: entity.primary_prefix,
  };
}
