export const LOGGER_IMAGE_URL_STORAGE_KEY = 'log73.logger_image_url';
export const DEFAULT_LOGGER_IMAGE_URL = 'https://www.hamqsl.com/solarn0nbh.php';
export const LOGGER_IMAGE_REFRESH_QUERY_PARAM = 'log73_refresh_hour';

export function normalizeLoggerImageUrl(url) {
  const normalizedUrl = String(url ?? '').trim();
  return normalizedUrl || DEFAULT_LOGGER_IMAGE_URL;
}

export function loadLoggerImageUrl() {
  if (typeof localStorage === 'undefined') {
    return DEFAULT_LOGGER_IMAGE_URL;
  }

  return normalizeLoggerImageUrl(
    localStorage.getItem(LOGGER_IMAGE_URL_STORAGE_KEY),
  );
}

export function saveLoggerImageUrl(url) {
  if (typeof localStorage === 'undefined') {
    return;
  }

  localStorage.setItem(
    LOGGER_IMAGE_URL_STORAGE_KEY,
    normalizeLoggerImageUrl(url),
  );
}

export function loggerImageRefreshUrl(url, timestamp = Date.now()) {
  const normalizedUrl = String(url ?? '').trim();
  if (!normalizedUrl) {
    return '';
  }

  const hashIndex = normalizedUrl.indexOf('#');
  const withoutHash =
    hashIndex >= 0 ? normalizedUrl.slice(0, hashIndex) : normalizedUrl;
  const hash = hashIndex >= 0 ? normalizedUrl.slice(hashIndex) : '';
  const queryIndex = withoutHash.indexOf('?');
  const base = queryIndex >= 0 ? withoutHash.slice(0, queryIndex) : withoutHash;
  const query = queryIndex >= 0 ? withoutHash.slice(queryIndex + 1) : '';
  const params = new URLSearchParams(query);
  const hourBucket = Math.floor(Number(timestamp) / (60 * 60 * 1000));

  params.set(LOGGER_IMAGE_REFRESH_QUERY_PARAM, String(hourBucket));

  const nextQuery = params.toString();
  return `${base}${nextQuery ? `?${nextQuery}` : ''}${hash}`;
}
