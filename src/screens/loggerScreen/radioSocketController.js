import { websocketUrl } from '../../lib/api.js';

export function radioSocketUrl(endpoint, { loggerId, logId, baseUrl } = {}) {
  const resolved = new URL(websocketUrl(endpoint, {}, baseUrl));
  if (!resolved.searchParams.has('logger_id')) {
    resolved.searchParams.set('logger_id', String(loggerId));
  }
  if (!resolved.searchParams.has('log_id')) {
    resolved.searchParams.set('log_id', String(logId));
  }
  return resolved.toString();
}
