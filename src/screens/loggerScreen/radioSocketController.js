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

export function wsjtxTargetUpdate(message, { loggerId, logId, targetIntent }) {
  const targetLoggerId = String(message?.logger_id ?? '');
  if (!targetLoggerId) {
    return { isTarget: false, reclaimTarget: Boolean(targetIntent) };
  }
  return {
    isTarget:
      targetLoggerId === String(loggerId) && Number(message?.log_id) === logId,
    reclaimTarget: false,
  };
}

export function wsjtxReceiptMessage(message, logId, accepted) {
  if (
    accepted === false ||
    Number(message?.log_id) !== logId ||
    !String(message?.event_id ?? '').trim()
  ) {
    return null;
  }
  return {
    type: 'wsjtx_event_received',
    event_id: message.event_id,
  };
}
