import assert from 'node:assert/strict';
import test from 'node:test';
import { loadCallsignFixture } from '../../tests/fixtures/domain/loadFixture.js';
import {
  STRUCTURE_EDGE_SLASH,
  STRUCTURE_INVALID_CHARACTER,
  STRUCTURE_OK,
  STRUCTURE_TOO_MANY_SLASHES,
  checkCallsignStructure,
  dupeIdentity,
  isCallsignLike,
  normalizeCallsign,
  parseCallsign,
} from './callsign.js';

test('shared fixture base, modifier, and identity match', () => {
  const fixture = loadCallsignFixture();
  assert.equal(fixture.version, 1);
  assert.ok(fixture.cases.length > 0);
  for (const kase of fixture.cases) {
    const parts = parseCallsign(kase.input);
    assert.equal(parts.base, kase.base, `base for ${kase.input}`);
    assert.equal(parts.modifier, kase.modifier, `modifier for ${kase.input}`);
    assert.equal(
      dupeIdentity(kase.input),
      kase.dupe_identity,
      `identity for ${kase.input}`,
    );
  }
});

test('normalizeCallsign trims and uppercases', () => {
  assert.equal(normalizeCallsign('  dl/w1aw  '), 'DL/W1AW');
  assert.equal(normalizeCallsign(null), '');
});

test('isCallsignLike requires a digit in the 2nd or 3rd character', () => {
  assert.equal(isCallsignLike('K1ABC'), true);
  assert.equal(isCallsignLike('VE3'), true);
  assert.equal(isCallsignLike('W1?'), true);
  assert.equal(isCallsignLike('DL'), false);
  assert.equal(isCallsignLike('P'), false);
  assert.equal(isCallsignLike(''), false);
});

test('parseCallsign resolves single callsigns without a modifier', () => {
  assert.deepEqual(parseCallsign('K1ABC'), {
    normalized: 'K1ABC',
    base: 'K1ABC',
    modifier: null,
  });
});

test('parseCallsign normalizes whitespace and case before resolving', () => {
  assert.deepEqual(parseCallsign('  dl/w1aw  '), {
    normalized: 'DL/W1AW',
    base: 'W1AW',
    modifier: 'DL',
  });
});

test('parseCallsign resolves prefix form to the home station', () => {
  for (const input of ['DL/W1AW', 'F/W1AW', 'PA/W1AW', 'EA/W1AW', 'G/W1AW']) {
    const parts = parseCallsign(input);
    assert.equal(parts.base, 'W1AW', `base for ${input}`);
    assert.equal(dupeIdentity(input), 'W1AW');
  }
});

test('parseCallsign resolves suffix form to the left component', () => {
  for (const [input, base] of [
    ['K1ABC/P', 'K1ABC'],
    ['N0CALL/M', 'N0CALL'],
    ['W3AAA/MM', 'W3AAA'],
    ['K1ABC/QRP', 'K1ABC'],
    ['W9ABC/4', 'W9ABC'],
    ['NG4M/1', 'NG4M'],
    ['NG4M/XYZ', 'NG4M'],
  ]) {
    const parts = parseCallsign(input);
    assert.equal(parts.base, base, `base for ${input}`);
    assert.equal(dupeIdentity(input), base);
  }
});

test('parseCallsign resolves ambiguous pairs by length with right tiebreak', () => {
  assert.equal(parseCallsign('VP2M/K1ABC').base, 'K1ABC');
  assert.equal(parseCallsign('K1ABC/VE3').base, 'K1ABC');
  assert.equal(parseCallsign('KL7RA/WK9').base, 'KL7RA');
  assert.equal(parseCallsign('EA8/K1A').base, 'K1A');
  // Order-independent: both slash orders identify the same station.
  assert.equal(parseCallsign('W1AW/VE3').base, parseCallsign('VE3/W1AW').base);
});

test('distinct operators sharing a prefix never collide', () => {
  assert.notEqual(dupeIdentity('VE3/K1ABC'), dupeIdentity('VE3/N2XYZ'));
  assert.equal(dupeIdentity('VE3/K1ABC'), 'K1ABC');
  assert.equal(dupeIdentity('VE3/N2XYZ'), 'N2XYZ');
});

test('dupeIdentity falls back to the full input when unresolvable', () => {
  for (const input of ['AB/CD', '/K1ABC', 'K1ABC/', 'K1/A/BC', 'A/B/C']) {
    assert.equal(parseCallsign(input).base, null, `base for ${input}`);
    assert.equal(dupeIdentity(input), normalizeCallsign(input));
  }
  assert.equal(parseCallsign('').base, '');
  assert.equal(dupeIdentity(''), '');
});

test('checkCallsignStructure reports charset and slash placement', () => {
  assert.equal(checkCallsignStructure('K1ABC', false), STRUCTURE_OK);
  assert.equal(checkCallsignStructure('DL/W1AW', false), STRUCTURE_OK);
  assert.equal(
    checkCallsignStructure('K1A*', false),
    STRUCTURE_INVALID_CHARACTER,
  );
  assert.equal(
    checkCallsignStructure('W1?', false),
    STRUCTURE_INVALID_CHARACTER,
  );
  assert.equal(checkCallsignStructure('W1?', true), STRUCTURE_OK);
  assert.equal(
    checkCallsignStructure('K1/A/BC', false),
    STRUCTURE_TOO_MANY_SLASHES,
  );
  assert.equal(checkCallsignStructure('/K1ABC', false), STRUCTURE_EDGE_SLASH);
  assert.equal(checkCallsignStructure('K1ABC/', false), STRUCTURE_EDGE_SLASH);
});
