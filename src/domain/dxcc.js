function normalizeCallsign(callsign) {
  return String(callsign ?? '')
    .trim()
    .toUpperCase();
}

export function callsignPrefix(callsign) {
  const normalized = normalizeCallsign(callsign);
  if (!normalized || !/^[A-Z0-9/]+$/.test(normalized)) return null;

  let source = normalized;
  if (normalized.includes('/')) {
    const parts = normalized.split('/');
    if (parts.length !== 2 || !parts[0] || !parts[1]) return null;

    const [left, right] = parts;
    if (isNonPrefixCallsignDesignator(right)) {
      source = left;
    } else if (/^\d+$/.test(right)) {
      source = portableNumericPrefix(left, right);
      if (!source) return null;
    } else {
      const leftPrefixLike = isPrefixLikeCallsignComponent(left);
      const rightPrefixLike = isPrefixLikeCallsignComponent(right);
      if (leftPrefixLike && !rightPrefixLike) {
        source = left;
      } else if (!leftPrefixLike && rightPrefixLike) {
        source = right;
      } else {
        source = left.length < right.length ? left : right;
      }
    }
  }

  return prefixFromComponent(source);
}

function isNonPrefixCallsignDesignator(value) {
  return [
    'MM',
    'M',
    'AM',
    'A',
    'E',
    'J',
    'P',
    'QRP',
    'QRPP',
    'AG',
    'AE',
    'KT',
  ].includes(value);
}

function isPrefixLikeCallsignComponent(value) {
  return !/\d/.test(value) || /\d$/.test(value);
}

function portableNumericPrefix(base, portableNumber) {
  const lastDigit = base.search(/\d(?!.*\d)/);
  if (lastDigit < 0) return null;
  let digitGroupStart = lastDigit;
  while (digitGroupStart > 0 && /\d/.test(base[digitGroupStart - 1])) {
    digitGroupStart -= 1;
  }
  const stem = base.slice(0, digitGroupStart);
  return stem ? `${stem}${portableNumber}` : null;
}

function prefixFromComponent(component) {
  if (!component || !/^[A-Z0-9]+$/.test(component)) return null;
  const lastDigit = component.search(/\d(?!.*\d)/);
  if (lastDigit >= 0) return component.slice(0, lastDigit + 1);

  const letters = [...component]
    .filter((character) => /[A-Z]/.test(character))
    .slice(0, 2)
    .join('');
  return letters.length === 2 ? `${letters}0` : null;
}

export function lookupDxcc(database, callsign) {
  // Keep this slash-callsign DXCC resolution logic in sync with
  // backend/src/dxcc.rs when changing either side.
  const normalizedCallsign = normalizeCallsign(callsign);
  if (!normalizedCallsign) return null;

  const exactMatch = exactDxccRule(database, normalizedCallsign);
  if (exactMatch) return dxccInfoForRule(exactMatch, database?.entities);

  const slashParts = splitSlashCallsign(normalizedCallsign);
  if (!slashParts) return lookupDxccDirect(database, normalizedCallsign);

  const { left, right } = slashParts;
  if (left.length < right.length) {
    return lookupDxccDirect(database, left);
  }

  if (isIgnoredSlashSuffix(right)) {
    return lookupDxccDirect(database, left);
  }

  return lookupDxccDirect(database, right) ?? lookupDxccDirect(database, left);
}

function lookupDxccDirect(database, callsign) {
  const normalizedCallsign = normalizeCallsign(callsign);
  if (!normalizedCallsign) return null;

  const exactMatch = exactDxccRule(database, normalizedCallsign);
  if (exactMatch) return dxccInfoForRule(exactMatch, database?.entities);

  if (!callsignPrefix(normalizedCallsign)) return null;

  const rules = Array.isArray(database?.rules) ? database.rules : [];
  const entities = Array.isArray(database?.entities) ? database.entities : [];
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

function exactDxccRule(database, normalizedCallsign) {
  const rules = Array.isArray(database?.rules) ? database.rules : [];
  const entities = Array.isArray(database?.entities) ? database.entities : [];
  let bestRule = null;
  for (const rule of rules) {
    if (rule?.exact !== true || rule.pattern !== normalizedCallsign) continue;
    const entity = entities[rule.entity_index];
    const bestEntity = entities[bestRule?.entity_index];
    if (!bestRule || (entity?.waedc_cq_list && !bestEntity?.waedc_cq_list)) {
      bestRule = rule;
    }
  }
  return bestRule;
}

function splitSlashCallsign(callsign) {
  const slashIndex = callsign.indexOf('/');
  if (slashIndex <= 0 || slashIndex !== callsign.lastIndexOf('/')) return null;
  if (slashIndex >= callsign.length - 1) return null;

  return {
    left: callsign.slice(0, slashIndex),
    right: callsign.slice(slashIndex + 1),
  };
}

function isIgnoredSlashSuffix(part) {
  return (
    part === 'M' ||
    part === 'P' ||
    part === 'MM' ||
    part === 'QRP' ||
    /^\d$/.test(part)
  );
}

export function dxccLabel(dxccInfo) {
  const countryName = String(dxccInfo?.country_name ?? '').trim();
  const continent = String(dxccInfo?.continent ?? '')
    .trim()
    .toUpperCase();
  if (!countryName || !continent) return '';
  return `${countryName} ${continent}`;
}

export function dxccContinent(dxccInfo) {
  const continent = String(dxccInfo?.continent ?? '')
    .trim()
    .toUpperCase();
  return continent || null;
}

function dxccInfoForRule(rule, entities) {
  const entity = entities?.[rule?.entity_index];
  if (!entity) return null;

  return {
    country_name: entity.country_name,
    adif: entity.adif,
    cq_zone: rule.cq_zone ?? entity.cq_zone,
    itu_zone: rule.itu_zone ?? entity.itu_zone,
    continent: rule.continent ?? entity.continent,
    latitude: rule.latitude ?? entity.latitude,
    longitude: rule.longitude ?? entity.longitude,
    utc_offset: rule.utc_offset ?? entity.utc_offset,
    primary_prefix: entity.primary_prefix,
    waedc_cq_list: Boolean(entity.waedc_cq_list),
  };
}
