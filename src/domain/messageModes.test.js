import assert from 'node:assert/strict';
import test from 'node:test';
import {
  normalizeMessageMode,
  parseMessageModeSectionHeader,
} from './messageModes.js';
import { loadMessageModesFixture } from '../../tests/fixtures/domain/loadFixture.js';

const fixture = loadMessageModesFixture();
test('shared message mode contract has the supported schema and cases', () => {
  assert.equal(fixture.version, 1);
  assert.ok(fixture.normalization_cases.length > 0);
  assert.ok(fixture.header_cases.length > 0);
});
for (const c of fixture.normalization_cases) {
  test(`shared message mode: ${c.name}`, () => {
    assert.equal(normalizeMessageMode(c.input), c.normalized);
  });
}
for (const c of fixture.header_cases) {
  test(`shared message header: ${c.name}`, () => {
    assert.equal(parseMessageModeSectionHeader(c.input), c.mode);
  });
}
