import assert from 'node:assert/strict';
import test from 'node:test';
import {
  NONE_SOUND_DEVICE_ID,
  filterSoundDevicesByHost,
  normalizeSoundDeviceId,
  preferredSoundDeviceHost,
  soundDeviceOptionLabel,
  soundDeviceHosts,
  soundDeviceOptions,
} from './soundDevices.js';

test('normalizeSoundDeviceId trims values and maps blank selections to null', () => {
  assert.equal(normalizeSoundDeviceId(null), null);
  assert.equal(normalizeSoundDeviceId(undefined), null);
  assert.equal(normalizeSoundDeviceId(''), null);
  assert.equal(normalizeSoundDeviceId('   '), null);
  assert.equal(normalizeSoundDeviceId(' alsa:hw:1,0 '), 'alsa:hw:1,0');
});

test('soundDeviceOptionLabel includes the name and default marker', () => {
  assert.equal(
    soundDeviceOptionLabel({
      id: 'alsa:hw:1,0',
      host: 'alsa',
      name: 'USB Audio',
      is_default: true,
    }),
    'USB Audio (default)',
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
  assert.equal(options[2].label, 'Headphones (default)');
});

test('sound-device hosts are unique and can filter devices', () => {
  const devices = [
    { id: 'pipewire:in', host: 'pipewire' },
    { id: 'alsa:in', host: 'alsa' },
    { id: 'pipewire:out', host: 'pipewire' },
  ];

  assert.deepEqual(soundDeviceHosts(devices), ['alsa', 'pipewire']);
  assert.deepEqual(filterSoundDevicesByHost(devices, 'pipewire'), [
    devices[0],
    devices[2],
  ]);
  assert.deepEqual(
    filterSoundDevicesByHost(devices, NONE_SOUND_DEVICE_ID),
    devices,
  );
});

test('preferredSoundDeviceHost applies Linux subsystem preferences', () => {
  assert.equal(
    preferredSoundDeviceHost(
      ['alsa', 'pulseaudio', 'pipewire'],
      'Linux x86_64',
    ),
    'pipewire',
  );
  assert.equal(
    preferredSoundDeviceHost(['alsa'], 'MacIntel'),
    NONE_SOUND_DEVICE_ID,
  );
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
