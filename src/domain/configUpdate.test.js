import assert from 'node:assert/strict';
import test from 'node:test';
import { buildConfigUpdatePayload } from './configUpdate.js';

test('buildConfigUpdatePayload preserves auth when password fields are blank', () => {
  const payload = buildConfigUpdatePayload({
    loginUser: 'greg',
    loginPassword: '',
    loginPasswordConfirm: '',
    disableLogin: false,
    iaruRegion: '3',
    dxClusterEnabled: true,
    dxClusterHost: 'cluster.example.test',
    dxClusterPort: '7300',
    dxClusterCallsign: 'N0CALL',
    dxClusterMaxAgeMin: '120',
    dxClusterCommands: 'show/dx',
  });

  assert.deepEqual(payload, {
    iaru_region: 3,
    login_user: 'greg',
    disable_login: false,
    dxcluster_enabled: true,
    dxcluster_host: 'cluster.example.test',
    dxcluster_port: 7300,
    dxcluster_callsign: 'N0CALL',
    dxcluster_max_age_min: 120,
    dxcluster_commands: 'show/dx',
  });
});

test('buildConfigUpdatePayload sends password change fields when provided', () => {
  const payload = buildConfigUpdatePayload({
    loginUser: 'greg',
    loginPassword: 'secret',
    loginPasswordConfirm: 'secret',
    disableLogin: false,
    iaruRegion: '2',
    dxClusterEnabled: false,
    dxClusterHost: '',
    dxClusterPort: '23',
    dxClusterCallsign: '',
    dxClusterMaxAgeMin: '60',
    dxClusterCommands: '',
  });

  assert.equal(payload.login_password_change, 'secret');
  assert.equal(payload.login_password_confirm, 'secret');
  assert.equal(payload.disable_login, false);
});

test('buildConfigUpdatePayload sends explicit disable without password change fields', () => {
  const payload = buildConfigUpdatePayload({
    loginUser: 'greg',
    loginPassword: '',
    loginPasswordConfirm: '',
    disableLogin: true,
    iaruRegion: '1',
    dxClusterEnabled: false,
    dxClusterHost: '',
    dxClusterPort: '23',
    dxClusterCallsign: '',
    dxClusterMaxAgeMin: '60',
    dxClusterCommands: '',
  });

  assert.equal(payload.disable_login, true);
  assert.equal(payload.iaru_region, 1);
  assert.equal('login_password_change' in payload, false);
  assert.equal('login_password_confirm' in payload, false);
});

test('buildConfigUpdatePayload defaults invalid IARU region values to Region 2', () => {
  const payload = buildConfigUpdatePayload({
    loginUser: '',
    loginPassword: '',
    loginPasswordConfirm: '',
    disableLogin: true,
    iaruRegion: '',
    dxClusterEnabled: false,
    dxClusterHost: '',
    dxClusterPort: '23',
    dxClusterCallsign: '',
    dxClusterMaxAgeMin: '60',
    dxClusterCommands: '',
  });

  assert.equal(payload.iaru_region, 2);
});
