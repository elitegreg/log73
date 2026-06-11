import assert from 'node:assert/strict';
import test from 'node:test';
import {
  NONE_SOUND_DEVICE_ID,
  normalizeSoundDeviceId,
  soundDeviceOptionLabel,
  soundDeviceOptions,
} from './soundDevices.js';

test('normalizeSoundDeviceId trims values and maps blank selections to null', () => {
  assert.equal(normalizeSoundDeviceId(null), null);
  assert.equal(normalizeSoundDeviceId(undefined), null);
  assert.equal(normalizeSoundDeviceId(''), null);
  assert.equal(normalizeSoundDeviceId('   '), null);
  assert.equal(normalizeSoundDeviceId(' alsa:hw:1,0 '), 'alsa:hw:1,0');
});

test('soundDeviceOptionLabel includes name host and default marker', () => {
  assert.equal(
    soundDeviceOptionLabel({
      id: 'alsa:hw:1,0',
      host: 'alsa',
      name: 'USB Audio',
      is_default: true,
    }),
    'USB Audio [alsa] (default)',
  );
  assert.equal(
    soundDeviceOptionLabel({ id: 'coreaudio:1', description: 'Line Out' }),
    'Line Out',
  );
  assert.equal(soundDeviceOptionLabel({}), 'Unknown sound device');
});

test('soundDeviceOptions always includes None first and de-duplicates device ids', () => {
  const options = soundDeviceOptions([
    { id: 'alsa:out-1', host: 'alsa', name: 'Line Out' },
    { id: 'alsa:out-1', host: 'alsa', name: 'Duplicate Line Out' },
    { id: '  ', host: 'alsa', name: 'Blank' },
    { id: 'alsa:out-2', host: 'alsa', name: 'Headphones', is_default: true },
  ]);

  assert.equal(options[0].id, NONE_SOUND_DEVICE_ID);
  assert.equal(options[0].label, 'None');
  assert.deepEqual(
    options.map((option) => option.id),
    ['', 'alsa:out-1', 'alsa:out-2'],
  );
  assert.equal(options[2].label, 'Headphones [alsa] (default)');
});

test('soundDeviceOptions preserves missing selected device so forms remain controlled', () => {
  const options = soundDeviceOptions(
    [{ id: 'alsa:out-1', host: 'alsa', name: 'Line Out' }],
    'alsa:missing',
  );

  assert.deepEqual(
    options.map((option) => option.id),
    ['', 'alsa:out-1', 'alsa:missing'],
  );
  assert.equal(options[2].label, 'alsa:missing (not found)');
});
