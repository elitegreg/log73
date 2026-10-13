// Shared exchange-applicability contract.
//
// This module is the JS half of the `only_when` contract; its mirror is
// `contact_matches_condition` in `backend/src/validation.rs`. Keep the two
// implementations behaviorally identical. The contract is locked by
// `tests/fixtures/domain/conditions.json`, which both languages execute
// directly (see `tests/fixtures/domain/README.md`).
//
// Clause order and semantics match the backend exactly: the source value is
// read from ADIF first, then metadata; an empty source never applies;
// `values` is enforced only when non-empty; `exclude_values`,
// `matches_field`, and `not_matches_field` all apply as AND clauses.
//
// Suffix predicates (`suffixes`/`exclude_suffixes`) belong to scoring's
// condition evaluator and are deliberately ignored here: copying that
// evaluator would change exchange behavior. A future extension must update
// both implementations and tests together.

function scalarString(value) {
  if (
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean'
  ) {
    return String(value).trim().toUpperCase();
  }
  // Non-scalars count as absent, matching the backend's trimmed-string lookup.
  return null;
}

function normalizeConditionValue(value) {
  return scalarString(value) ?? '';
}

function rawAdifValue(source, normalizedField) {
  if (source === null || typeof source !== 'object') return undefined;
  for (const key of Object.keys(source)) {
    if (key.toUpperCase() === normalizedField) return source[key];
  }
  return undefined;
}

function rawMetaValue(source, field) {
  if (source === null || typeof source !== 'object') return undefined;
  const key = typeof field === 'string' ? field : String(field ?? '');
  return Object.hasOwn(source, key) ? source[key] : undefined;
}

// ADIF first (normalized, case-insensitive keys), then metadata (verbatim,
// case-sensitive key) — exactly like the backend lookups. A present scalar
// (even blank) wins; a missing or non-scalar value falls through to the next
// source.
function lookupConditionValue(field, fields, meta) {
  const normalizedField = normalizeConditionValue(field);
  if (normalizedField === '') return '';
  const adifRaw = rawAdifValue(fields, normalizedField);
  if (adifRaw !== undefined) {
    const value = scalarString(adifRaw);
    if (value !== null) return value;
  }
  const metaRaw = rawMetaValue(meta, field);
  if (metaRaw !== undefined) {
    const value = scalarString(metaRaw);
    if (value !== null) return value;
  }
  return '';
}

// Report whether an `only_when` condition applies given field and metadata
// maps. `fields` and `meta` mirror a contact's ADIF and metadata values.
export function conditionApplies(condition, fields = {}, meta = {}) {
  const value = lookupConditionValue(condition?.field, fields, meta);
  if (value === '') return false;

  const values = condition?.values ?? [];
  if (
    values.length > 0 &&
    !values.some((candidate) => normalizeConditionValue(candidate) === value)
  ) {
    return false;
  }

  const excludeValues = condition?.exclude_values ?? [];
  if (
    excludeValues.some(
      (candidate) => normalizeConditionValue(candidate) === value,
    )
  ) {
    return false;
  }

  if (condition?.matches_field != null) {
    const other = lookupConditionValue(condition.matches_field, fields, meta);
    if (other === '' || other !== value) return false;
  }

  if (condition?.not_matches_field != null) {
    const other = lookupConditionValue(
      condition.not_matches_field,
      fields,
      meta,
    );
    if (other === '' || other === value) return false;
  }

  return true;
}
