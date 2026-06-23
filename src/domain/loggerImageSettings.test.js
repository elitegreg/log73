import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_LOGGER_IMAGE_URL,
  LOGGER_IMAGE_URL_STORAGE_KEY,
  loadLoggerImageUrl,
  loggerImageRefreshUrl,
  normalizeLoggerImageUrl,
  saveLoggerImageUrl,
} from './loggerImageSettings.js';

function withMockLocalStorage(callback) {
  const originalLocalStorage = globalThis.localStorage;
  const values = new Map();

  globalThis.localStorage = {
    getItem(key) {
      return values.has(key) ? values.get(key) : null;
    },
    setItem(key, value) {
      values.set(String(key), String(value));
    },
    removeItem(key) {
      values.delete(String(key));
    },
  };

  try {
    callback(values);
  } finally {
    if (originalLocalStorage === undefined) {
      delete globalThis.localStorage;
    } else {
      globalThis.localStorage = originalLocalStorage;
    }
  }
}

test('normalizeLoggerImageUrl falls back to default url for empty input', () => {
  assert.equal(normalizeLoggerImageUrl(''), DEFAULT_LOGGER_IMAGE_URL);
  assert.equal(normalizeLoggerImageUrl('   '), DEFAULT_LOGGER_IMAGE_URL);
  assert.equal(
    normalizeLoggerImageUrl('https://example.com/image.png'),
    'https://example.com/image.png',
  );
});

test('load and save logger image url use localStorage with normalization', () => {
  withMockLocalStorage((values) => {
    saveLoggerImageUrl('https://example.com/current.png');
    assert.equal(
      values.get(LOGGER_IMAGE_URL_STORAGE_KEY),
      'https://example.com/current.png',
    );
    assert.equal(loadLoggerImageUrl(), 'https://example.com/current.png');

    saveLoggerImageUrl('   ');
    assert.equal(
      values.get(LOGGER_IMAGE_URL_STORAGE_KEY),
      DEFAULT_LOGGER_IMAGE_URL,
    );
    assert.equal(loadLoggerImageUrl(), DEFAULT_LOGGER_IMAGE_URL);
  });
});

test('loadLoggerImageUrl returns default url when localStorage is unavailable', () => {
  const originalLocalStorage = globalThis.localStorage;
  delete globalThis.localStorage;
  try {
    assert.equal(loadLoggerImageUrl(), DEFAULT_LOGGER_IMAGE_URL);
  } finally {
    if (originalLocalStorage !== undefined) {
      globalThis.localStorage = originalLocalStorage;
    }
  }
});

test('loggerImageRefreshUrl applies hourly cache-buster and preserves hash', () => {
  const timestamp = 3 * 60 * 60 * 1000;
  const refreshed = loggerImageRefreshUrl(
    'https://example.com/solar.php?foo=bar#section',
    timestamp,
  );

  assert.equal(
    refreshed,
    'https://example.com/solar.php?foo=bar&log73_refresh_hour=3#section',
  );

  const replaced = loggerImageRefreshUrl(
    'https://example.com/solar.php?foo=bar&log73_refresh_hour=1',
    timestamp,
  );
  assert.equal(
    replaced,
    'https://example.com/solar.php?foo=bar&log73_refresh_hour=3',
  );
});
