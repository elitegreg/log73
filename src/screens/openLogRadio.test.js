import assert from 'node:assert/strict';
import test from 'node:test';
import {
  preferredRadioSelection,
  radioOwnershipLabel,
  radioSelectionDetail,
} from './openLogRadio.js';

test('client radio labels and details make ownership explicit', () => {
  assert.equal(
    radioOwnershipLabel({ control_location: 'backend' }),
    '[SERVER-SIDE]',
  );
  assert.equal(
    radioOwnershipLabel({ control_location: 'client', client_online: true }),
    '[CLIENT-SIDE · ONLINE]',
  );
  assert.match(
    radioSelectionDetail({ control_location: 'client', client_online: false }),
    /Start Log73 Radio Client/,
  );
});

test('initial radio selection avoids an offline client radio when possible', () => {
  const radios = [
    { id: 1, control_location: 'client', client_online: false },
    { id: 2, control_location: 'backend' },
  ];
  assert.equal(preferredRadioSelection('', radios), '2');
  assert.equal(preferredRadioSelection('1', radios), '1');
  assert.equal(preferredRadioSelection('', radios.slice(0, 1)), '1');
});
