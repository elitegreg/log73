const SOCKET_READY_STATE_LABELS = ['connecting', 'open', 'closing', 'closed'];
const MAX_SOCKET_DEBUG_DETAILS_LENGTH = 240;
const SOCKET_DEBUG_PANEL_QUERY_PARAM = 'socket_debug';
const SOCKET_DEBUG_PANEL_STORAGE_KEY = 'log73.socket_debug_panel';

export function websocketReadyStateLabel(readyState) {
  return SOCKET_READY_STATE_LABELS[readyState] ?? `unknown(${readyState})`;
}

export function formatSocketDebugTimestamp(timestamp) {
  return new Date(timestamp).toISOString().slice(11, 23);
}

export function formatSocketDebugDetails(details) {
  try {
    const text = JSON.stringify(details);
    if (!text || text === '{}') return '';
    return text.length <= MAX_SOCKET_DEBUG_DETAILS_LENGTH
      ? text
      : `${text.slice(0, MAX_SOCKET_DEBUG_DETAILS_LENGTH)}...`;
  } catch {
    return '[unserializable details]';
  }
}

export function readSocketDebugPanelEnabled(win = window) {
  if (!win) return false;

  try {
    const params = new URLSearchParams(win.location.search);
    const queryValue = params.get(SOCKET_DEBUG_PANEL_QUERY_PARAM);
    if (queryValue === '1') {
      win.localStorage?.setItem(SOCKET_DEBUG_PANEL_STORAGE_KEY, '1');
      return true;
    }
    if (queryValue === '0') {
      win.localStorage?.removeItem(SOCKET_DEBUG_PANEL_STORAGE_KEY);
      return false;
    }
    return win.localStorage?.getItem(SOCKET_DEBUG_PANEL_STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}
