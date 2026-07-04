export function normalizeAdifFieldName(name) {
  return String(name ?? '')
    .trim()
    .toUpperCase()
    .replace(/[^A-Z0-9_]/g, '_');
}

export function parseFirstAdifRecord(text) {
  const source = String(text ?? '');
  let index = 0;
  let inHeader = true;
  let pending = {};
  let current = {};

  while (index < source.length) {
    const tagStart = source.indexOf('<', index);
    if (tagStart === -1) break;
    const tagEnd = source.indexOf('>', tagStart + 1);
    if (tagEnd === -1) break;

    const tag = source.slice(tagStart + 1, tagEnd).trim();
    const normalizedTag = tag.toUpperCase();
    index = tagEnd + 1;

    if (normalizedTag === 'EOH') {
      inHeader = false;
      pending = {};
      continue;
    }
    if (normalizedTag === 'EOR') {
      const fields = inHeader ? pending : current;
      return { fields };
    }

    const parsedTag = parseFieldTag(tag);
    if (!parsedTag) continue;

    const value = source.slice(index, index + parsedTag.length).trim();
    index += parsedTag.length;

    if (inHeader) {
      pending[parsedTag.name] = value;
    } else {
      current[parsedTag.name] = value;
    }
  }

  if (Object.keys(current).length > 0) return { fields: current };
  if (Object.keys(pending).length > 0) return { fields: pending };
  return { fields: {} };
}

export function adifFieldOptions(fields) {
  return Object.entries(fields ?? {})
    .map(([name, value]) => ({
      name: normalizeAdifFieldName(name),
      value: String(value ?? ''),
    }))
    .filter((field) => field.name)
    .sort((left, right) => left.name.localeCompare(right.name));
}

export function adifFieldOptionLabel(field) {
  const value = String(field?.value ?? '').replace(/\s+/g, ' ').trim();
  if (!value) return field.name;
  return `${field.name} (e.g. '${value.slice(0, 40)}')`;
}

export function fixedValueMappingErrors(fields, mappings) {
  return (fields ?? [])
    .filter((field) => {
      const mapping = mappings?.[field.adif];
      return (
        mapping?.kind === 'fixed_value' &&
        String(mapping.value ?? '').trim() === ''
      );
    })
    .map((field) => ({
      error: `${field.name} fixed value is required`,
    }));
}

function parseFieldTag(tag) {
  const parts = tag.split(':');
  if (parts.length < 2) return null;
  const name = normalizeAdifFieldName(parts[0]);
  const length = Number.parseInt(parts[parts.length - 1], 10);
  if (!name || !Number.isFinite(length) || length < 0) return null;
  return { name, length };
}
