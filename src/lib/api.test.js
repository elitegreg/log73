import test from 'node:test';
import assert from 'node:assert/strict';
import { apiJson, apiDownload, websocketUrl } from './api.js';

function respond(t, body, options = {}) {
  t.mock.method(globalThis, 'fetch', async () => new Response(body, options));
}

test('apiJson preserves single-field, multiple-field, and scalar success bodies', async (t) => {
  for (const payload of [
    { login_user: 'greg' },
    { contact: { meta: { id: 1 } }, contacts: [{ meta: { id: 1 } }] },
    { imported: 1, contacts: [] },
    { next: 10, next_by_band: {}, reservation_required: false },
    { cw: [], digital: [], voice: [] },
    { ok: true, domain_field: 1 },
    ['CW', 'SSB'],
    'message defaults',
    true,
    {},
  ]) {
    respond(t, JSON.stringify(payload), {
      headers: { 'content-type': 'application/json' },
    });
    assert.deepEqual(await apiJson('/resource'), payload);
  }
});

test('apiJson preserves structured import and outbox error details', async (t) => {
  for (const payload of [
    {
      error: 'invalid record',
      line: 3,
      errors: [{ line: 3, error: 'invalid record' }],
    },
    { error: 'contact 1: serial is required', status: 'failed' },
  ]) {
    respond(t, JSON.stringify(payload), {
      status: 400,
      headers: { 'content-type': 'application/json' },
    });
    await assert.rejects(
      () => apiJson('/logs/1/contacts'),
      (error) => {
        assert.equal(error.message, payload.error);
        assert.deepEqual(error.payload, payload);
        return true;
      },
    );
  }
});

test('API helpers report JSON, malformed JSON, and non-JSON failures', async (t) => {
  for (const request of [apiJson, apiDownload]) {
    for (const [body, contentType, message] of [
      ['{"error":"missing resource"}', 'application/json', 'missing resource'],
      ['{broken', 'application/json', 'request failed: 500'],
      ['{}', 'application/json', 'request failed: 500'],
      ['  proxy unavailable  ', 'text/plain', 'proxy unavailable'],
      ['', 'text/plain', 'request failed: 500'],
    ]) {
      respond(t, body, {
        status: 500,
        headers: { 'content-type': contentType },
      });
      await assert.rejects(
        () => request('/resource'),
        (error) => {
          assert.equal(error.message, message);
          return true;
        },
      );
    }
  }
});

test('apiJson returns null for 204 and empty success bodies', async (t) => {
  respond(t, null, { status: 204 });
  assert.equal(await apiJson('/client-errors', { method: 'POST' }), null);
  respond(t, '');
  assert.equal(await apiJson('/empty'), null);
});

test('apiJson rejects malformed successful JSON', async (t) => {
  respond(t, '{broken');
  await assert.rejects(() => apiJson('/resource'), SyntaxError);
});

test('apiDownload preserves contents and filename, with a fallback filename', async (t) => {
  for (const [headers, expected] of [
    [
      { 'content-disposition': 'attachment; filename="N0CALL.adi"' },
      'N0CALL.adi',
    ],
    [{}, 'download.log'],
  ]) {
    respond(t, 'ADIF content', { headers });
    const { blob, filename } = await apiDownload('/logs/1/adif', {
      method: 'POST',
    });
    assert.equal(await blob.text(), 'ADIF content');
    assert.equal(filename, expected);
  }
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
