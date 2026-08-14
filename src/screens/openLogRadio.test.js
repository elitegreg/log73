import assert from 'node:assert/strict';
import test from 'node:test';
import {
  preferredRadioSelection,
  radioOwnershipLabel,
  visibleRadioOptions,
} from './openLogRadio.js';

test('client radio labels make remote ownership explicit', () => {
  assert.equal(radioOwnershipLabel({ control_location: 'backend' }), '');
  assert.equal(
    radioOwnershipLabel({ control_location: 'client', client_online: true }),
    '[Remote Client]',
  );
});

test('offline client radios are hidden and cannot remain selected', () => {
  const radios = [
    { id: 1, control_location: 'client', client_online: false },
    { id: 2, control_location: 'backend' },
    { id: 3, control_location: 'client', client_online: true },
  ];
  assert.deepEqual(
    visibleRadioOptions(radios).map((radio) => radio.id),
    [2, 3],
  );
  assert.equal(preferredRadioSelection('', radios), '2');
  assert.equal(preferredRadioSelection('1', radios), '2');
  assert.equal(preferredRadioSelection('', radios.slice(0, 1)), '');
});
