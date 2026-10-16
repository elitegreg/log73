import assert from 'node:assert/strict';
import test from 'node:test';
import {
  LOGGER_MODE_OPTIONS,
  adifModeForLoggerMode,
  isSelectableMode,
  modeIsCw,
  modeIsPhone,
  modeIsDigital,
  normalizeLoggerMode,
} from './modes.js';
import { loadModesFixture } from '../../tests/fixtures/domain/loadFixture.js';

const fixture = loadModesFixture();
test('shared logger mode contract has the supported schema and selectable order', () => {
  assert.equal(fixture.version, 1);
  assert.ok(fixture.cases.length > 0);
  assert.ok(fixture.logger_modes.length > 0);
  assert.deepEqual(LOGGER_MODE_OPTIONS, fixture.logger_modes);
});
for (const c of fixture.cases) {
  test(`shared logger mode: ${c.name}`, () => {
    assert.equal(normalizeLoggerMode(c.input), c.normalized);
    assert.equal(isSelectableMode(c.input), c.selectable);
    assert.equal(modeIsCw(c.input), c.cw);
    assert.equal(modeIsPhone(c.input), c.phone);
    assert.equal(modeIsDigital(c.input), c.digital);
    assert.equal(adifModeForLoggerMode(c.input), c.adif);
  });
}
