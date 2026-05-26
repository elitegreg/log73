function normalizeCallsign(callsign) {
  return String(callsign ?? '').trim().toUpperCase();
}

export function callsignPrefix(callsign) {
  const normalized = normalizeCallsign(callsign);
  if (!normalized) return null;

  const characters = [...normalized];
  if (/^\d$/.test(characters[0])) {
    const secondDigitIndex = characters.findIndex(
      (character, index) => index > 0 && /^\d$/.test(character),
    );
    return secondDigitIndex > 0
      ? characters.slice(0, secondDigitIndex).join('')
      : null;
  }

  const firstDigitIndex = characters.findIndex((character) =>
    /^\d$/.test(character),
  );
  return firstDigitIndex > 0
    ? characters.slice(0, firstDigitIndex).join('')
    : null;
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
  const continent = String(dxccInfo?.continent ?? '').trim().toUpperCase();
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
