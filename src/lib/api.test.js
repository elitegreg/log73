import test from 'node:test';
import assert from 'node:assert/strict';
import { apiJson, websocketUrl } from './api.js';

test('apiJson unwraps legacy ok payloads', async () => {
  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: async () => ({ ok: true, config: { login_user: 'greg' } }),
  });

  const result = await apiJson('/config');

  assert.deepEqual(result, { login_user: 'greg' });
});

test('apiJson throws backend json error messages for non-2xx responses', async () => {
  globalThis.fetch = async () => ({
    ok: false,
    status: 400,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: async () => ({ error: 'bad request details' }),
    text: async () => '',
  });

  await assert.rejects(() => apiJson('/config'), /bad request details/);
});

test('apiJson throws legacy ok false errors during transition', async () => {
  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: async () => ({ ok: false, error: 'legacy failure' }),
  });

  await assert.rejects(() => apiJson('/logs'), /legacy failure/);
});

test('apiJson returns null for 204 responses', async () => {
  globalThis.fetch = async () => ({
    ok: true,
    status: 204,
    headers: new Headers(),
  });

  const result = await apiJson('/client-errors', { method: 'POST' });

  assert.equal(result, null);
});

test('websocketUrl resolves a relative radio endpoint', () => {
  assert.equal(
    websocketUrl(
      '/radiows?radio_id=3',
      {},
      'https://logger.example/ui/logger/1/3',
    ),
    'wss://logger.example/radiows?radio_id=3',
  );
});

test('websocketUrl accepts a full HTTP radio endpoint and adds parameters', () => {
  assert.equal(
    websocketUrl(
      'http://localhost:7300/radiows',
      { radio_id: 3 },
      'https://logger.example/ui/logger/1/3',
    ),
    'ws://localhost:7300/radiows?radio_id=3',
  );
});
