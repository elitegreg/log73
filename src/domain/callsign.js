// Shared callsign parsing contract.
//
// This module is the JS half of the portable-callsign contract; its mirror is
// `backend/src/callsign.rs`. Keep the two implementations behaviorally
// identical. The contract is locked by `tests/fixtures/domain/callsigns.json`,
// which both languages execute directly (see
// `tests/fixtures/domain/README.md`).
//
// Three outputs are deliberately separate:
//
// - `base`: station identity, used for validation and duplicate detection.
// - WPX prefix (`callsignPrefix` in `./dxcc.js`): operating-prefix extraction.
// - DXCC entity (`lookupDxcc` in `./dxcc.js`): operating geography.
//
// A portable contact such as `VP2M/K1ABC` has dupe identity `K1ABC` but WPX
// prefix `VP2`; that disagreement is intended.

// Structural validation failure codes for a normalized callsign.
export const STRUCTURE_OK = null;
export const STRUCTURE_INVALID_CHARACTER = 'invalid-character';
export const STRUCTURE_TOO_MANY_SLASHES = 'too-many-slashes';
export const STRUCTURE_EDGE_SLASH = 'edge-slash';

// Trim surrounding whitespace and uppercase the input.
export function normalizeCallsign(value) {
  return String(value ?? '')
    .trim()
    .toUpperCase();
}

// Report whether a single component looks like a callsign: a digit in the
// 2nd or 3rd character. Callers pass normalized components.
export function isCallsignLike(component) {
  const second = String(component ?? '')[1] ?? '';
  const third = String(component ?? '')[2] ?? '';
  return /[0-9]/.test(second) || /[0-9]/.test(third);
}

// Check charset and slash placement of a normalized callsign. Returns one of
// the STRUCTURE_* codes above, or null when the structure is valid.
//
// Query mode additionally permits `?` wildcards. Length and required-ness are
// the caller's field-specific validation, not part of this check.
export function checkCallsignStructure(normalized, allowQuery = false) {
  const pattern = allowQuery ? /^[A-Z0-9/?]*$/ : /^[A-Z0-9/]*$/;
  if (!pattern.test(normalized)) return STRUCTURE_INVALID_CHARACTER;
  const slashCount = [...normalized].filter(
    (character) => character === '/',
  ).length;
  if (slashCount > 1) return STRUCTURE_TOO_MANY_SLASHES;
  if (normalized.startsWith('/') || normalized.endsWith('/')) {
    return STRUCTURE_EDGE_SLASH;
  }
  return STRUCTURE_OK;
}

// Resolve the input into `{ normalized, base, modifier }`. `base` is the
// station identity component, or null when components cannot be resolved (bad
// slash structure, or neither component looks like a callsign). `modifier` is
// the other slash component (operating prefix or suffix), or null when there
// is no slash or no resolution.
export function parseCallsign(value) {
  const normalized = normalizeCallsign(value);
  const slashIndex = normalized.indexOf('/');
  if (slashIndex === -1) {
    return { normalized, base: normalized, modifier: null };
  }
  if (
    slashIndex === 0 ||
    slashIndex !== normalized.lastIndexOf('/') ||
    slashIndex === normalized.length - 1
  ) {
    return { normalized, base: null, modifier: null };
  }
  const left = normalized.slice(0, slashIndex);
  const right = normalized.slice(slashIndex + 1);
  const [base, modifier] = resolvePair(left, right);
  return { normalized, base, modifier };
}

// Duplicate identity for any input, including legacy or forced values that the
// strict logging parser cannot parse: the resolved base when available, else
// the normalized full input so distinct values never collide.
export function dupeIdentity(value) {
  const parts = parseCallsign(value);
  return parts.base ?? parts.normalized;
}

function resolvePair(left, right) {
  const leftLike = isCallsignLike(left);
  const rightLike = isCallsignLike(right);
  // Operating prefix form: DL/W1AW identifies station W1AW.
  if (!leftLike && rightLike) return [right, left];
  // Operating suffix form: K1ABC/P, W9ABC/4, NG4M/XYZ identify the left.
  if (leftLike && !rightLike) return [left, right];
  if (leftLike && rightLike) {
    // Ambiguous: both components look like callsigns (VP2M/K1ABC,
    // K1ABC/VE3). The longer component is the station identity; ties
    // resolve to the right so W1AW/VE3 and VE3/W1AW agree.
    if ([...left].length > [...right].length) return [left, right];
    return [right, left];
  }
  return [null, null];
}
