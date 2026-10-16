import {
  BACKEND_WS_IDLE_PING_DELAY_MS,
  BACKEND_WS_INITIAL_RECONNECT_DELAY_MS,
  BACKEND_WS_MAX_RECONNECT_DELAY_MS,
  BACKEND_WS_PING_TIMEOUT_MS,
} from '../loggerScreenHelpers.js';

const CONNECTING = 0;
const OPEN = 1;
const CLOSING = 2;
const CLOSED = 3;

function browserEnvironment() {
  return {
    createSocket: (url) => new WebSocket(url),
    now: () => Date.now(),
    setTimeout: (callback, delay) => window.setTimeout(callback, delay),
    clearTimeout: (id) => window.clearTimeout(id),
    window,
    document,
    requestId: () =>
      window.crypto?.randomUUID?.() ??
      `${Date.now()}-${Math.random().toString(36).slice(2)}`,
  };
}

// A controller belongs to one URL/session. Callbacks and timers from retired
// connections cannot reach its replacement, even when close is asynchronous.
export function createReconnectingSocket({
  url,
  getCallbacks = () => ({}),
  createPing,
  isPong,
  connectTimeoutMs = 5000,
  initialReconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS,
  maxReconnectDelayMs = BACKEND_WS_MAX_RECONNECT_DELAY_MS,
  idlePingDelayMs = BACKEND_WS_IDLE_PING_DELAY_MS,
  pongTimeoutMs = BACKEND_WS_PING_TIMEOUT_MS,
  immediateTimeoutRecovery = false,
  foregroundConnectingGraceMs = null,
  environment = browserEnvironment(),
}) {
  const env = environment;
  const timers = new Map();
  let started = false;
  let disposed = false;
  let current = null;
  let reconnectDelayMs = initialReconnectDelayMs;
  let lastMessageAt = env.now();
  let pendingPingRequestId = null;

  const isCurrent = (connection) =>
    started && !disposed && current === connection;
  const notify = (name, ...args) => {
    if (!disposed) return getCallbacks()[name]?.(...args);
  };
  const debug = (event, details = {}) => notify('onDebug', event, details);

  function clearTimer(name) {
    const timer = timers.get(name);
    if (timer) env.clearTimeout(timer.id);
    timers.delete(name);
  }

  function setTimer(name, callback, delay, connection) {
    clearTimer(name);
    const timer = {};
    timers.set(name, timer);
    timer.id = env.setTimeout(() => {
      if (
        disposed ||
        timers.get(name) !== timer ||
        (connection && !isCurrent(connection))
      )
        return;
      timers.delete(name);
      callback();
    }, delay);
  }

  function clearHealth() {
    const clearedPendingPingRequestId = pendingPingRequestId;
    pendingPingRequestId = null;
    clearTimer('health');
    clearTimer('pong');
    return clearedPendingPingRequestId;
  }

  function closeSocket(connection) {
    try {
      connection.socket.close();
    } catch (error) {
      debug('socket_close_failed', { error: String(error) });
    }
  }

  function retire(connection, details = {}) {
    if (!isCurrent(connection)) return false;
    current = null;
    for (const [name, listener] of Object.entries(connection.listeners)) {
      connection.socket.removeEventListener(name, listener);
    }
    clearTimer('connect');
    clearTimer('foreground');
    const clearedPendingPingRequestId = clearHealth();
    debug('socket_close', { ...details, clearedPendingPingRequestId });
    notify('onStatusChange', 'disconnected');
    notify('onClose', details);
    closeSocket(connection);
    return true;
  }

  function scheduleReconnect() {
    if (disposed || timers.has('reconnect')) return;
    debug('reconnect_scheduled', { delayMs: reconnectDelayMs });
    setTimer(
      'reconnect',
      () => {
        debug('reconnect_timer_fired');
        connect();
      },
      reconnectDelayMs,
    );
    reconnectDelayMs = Math.min(reconnectDelayMs * 2, maxReconnectDelayMs);
  }

  function recover(connection, reason, immediate = immediateTimeoutRecovery) {
    if (!retire(connection, { reason })) return;
    if (disposed) return;
    if (immediate) {
      debug('reconnect_now', { reason });
      clearTimer('reconnect');
      reconnectDelayMs = initialReconnectDelayMs;
      connect();
    } else {
      scheduleReconnect();
    }
  }

  function send(connection, message) {
    if (
      !connection ||
      !isCurrent(connection) ||
      connection.socket.readyState !== OPEN
    )
      return false;
    try {
      connection.socket.send(JSON.stringify(message));
      return true;
    } catch (error) {
      notify('onError', error, { phase: 'send', url });
      recover(connection, 'send_failed');
      return false;
    }
  }

  function context(connection) {
    return {
      isCurrent: () => isCurrent(connection),
      send: (message) => send(connection, message),
    };
  }

  function scheduleHealth() {
    clearTimer('health');
    const connection = current;
    if (
      !connection ||
      !isCurrent(connection) ||
      env.document.hidden ||
      connection.socket.readyState !== OPEN ||
      pendingPingRequestId
    )
      return;
    setTimer(
      'health',
      () => checkHealth(),
      Math.max(idlePingDelayMs - (env.now() - lastMessageAt), 0),
      connection,
    );
  }

  function activity() {
    const clearedPendingPingRequestId = pendingPingRequestId;
    lastMessageAt = env.now();
    pendingPingRequestId = null;
    clearTimer('pong');
    scheduleHealth();
    return clearedPendingPingRequestId;
  }

  function checkHealth(source = 'timer', forcePing = false) {
    if (disposed || env.document.hidden) return;
    const connection = current;
    if (connection?.socket.readyState === CLOSED) {
      recover(connection, 'health_check_closed_socket', true);
      return;
    }
    if (connection?.socket.readyState === CLOSING) {
      recover(connection, 'health_check_closing_socket');
      return;
    }
    if (
      !connection ||
      connection.socket.readyState !== OPEN ||
      pendingPingRequestId
    )
      return;
    debug('health_check_started', {
      source,
      forcePing,
      idleMs: env.now() - lastMessageAt,
    });
    if (!forcePing && env.now() - lastMessageAt < idlePingDelayMs) {
      scheduleHealth();
      return;
    }
    clearTimer('health');
    const requestId = env.requestId();
    pendingPingRequestId = requestId;
    debug('ping_sent', { source, requestId, timeoutMs: pongTimeoutMs });
    if (!send(connection, createPing(requestId))) return;
    // A fake or embedded transport may deliver a response synchronously in send.
    if (!isCurrent(connection) || pendingPingRequestId !== requestId) return;
    setTimer(
      'pong',
      () => {
        if (pendingPingRequestId !== requestId) return;
        debug('pong_timeout', { requestId, timeoutMs: pongTimeoutMs });
        recover(connection, 'pong_timeout');
      },
      pongTimeoutMs,
      connection,
    );
  }

  function connect() {
    if (disposed || !started || current) return;
    clearTimer('reconnect');
    clearHealth();
    notify('onStatusChange', 'connecting');
    debug('connect_attempt', { url });
    if (disposed) return;
    let socket;
    try {
      socket = env.createSocket(url);
    } catch (error) {
      notify('onStatusChange', 'disconnected');
      notify('onError', error, { phase: 'connect', url });
      scheduleReconnect();
      return;
    }
    if (disposed) {
      closeSocket({ socket });
      return;
    }
    const connection = {
      socket,
      createdAt: env.now(),
      opened: false,
      listeners: {},
    };
    current = connection;
    connection.listeners = {
      open() {
        if (!isCurrent(connection) || connection.opened) return;
        connection.opened = true;
        clearTimer('connect');
        clearTimer('foreground');
        reconnectDelayMs = initialReconnectDelayMs;
        notify('onStatusChange', 'connected');
        activity();
        debug('socket_open');
        try {
          Promise.resolve(notify('onOpen', context(connection))).catch(
            (error) => {
              if (isCurrent(connection))
                notify('onError', error, { phase: 'open', url });
            },
          );
        } catch (error) {
          if (isCurrent(connection))
            notify('onError', error, { phase: 'open', url });
        }
      },
      message(event) {
        if (!isCurrent(connection)) return;
        const clearedPendingPingRequestId = activity();
        try {
          const message = JSON.parse(event.data);
          if (isPong(message)) {
            debug('pong_received', {
              requestId: message.request_id,
              clearedPendingPingRequestId,
            });
          } else if (clearedPendingPingRequestId) {
            debug('socket_activity_cleared_ping', {
              messageType: message?.type,
              clearedPendingPingRequestId,
            });
          }
          notify('onMessage', message, context(connection));
        } catch (error) {
          notify('onMessageError', error, event.data);
          debug('message_parse_failed');
        }
      },
      close(event) {
        if (!isCurrent(connection)) return;
        if (
          retire(connection, {
            code: event.code,
            reason: event.reason,
            wasClean: event.wasClean,
          })
        )
          scheduleReconnect();
      },
      error(event) {
        if (!isCurrent(connection)) return;
        debug('socket_error');
        notify('onError', event, { phase: 'socket', url });
        recover(connection, 'socket_error', false);
      },
    };
    for (const [name, listener] of Object.entries(connection.listeners))
      socket.addEventListener(name, listener);
    setTimer(
      'connect',
      () => {
        if (socket.readyState === OPEN) return;
        debug('connect_timeout', { timeoutMs: connectTimeoutMs });
        recover(connection, 'connect_timeout');
      },
      connectTimeoutMs,
      connection,
    );
  }

  function foreground(source) {
    if (disposed || env.document.hidden) return;
    debug('foreground_event', {
      source,
      visibilityState: env.document.visibilityState,
    });
    const connection = current;
    if (!connection || connection.socket.readyState === CLOSED) {
      if (connection)
        retire(connection, { reason: 'foreground_closed_socket' });
      clearTimer('reconnect');
      reconnectDelayMs = initialReconnectDelayMs;
      connect();
    } else if (connection.socket.readyState === CONNECTING) {
      if (foregroundConnectingGraceMs === null) {
        reconnectDelayMs = initialReconnectDelayMs;
        return;
      }
      const remaining =
        foregroundConnectingGraceMs - (env.now() - connection.createdAt);
      if (remaining > 0) {
        debug('foreground_connecting_grace', {
          source,
          graceRemainingMs: remaining,
        });
        setTimer(
          'foreground',
          () => foreground(`${source}:connecting_grace_elapsed`),
          remaining,
          connection,
        );
      } else {
        recover(connection, 'foreground_connecting_stalled', true);
      }
    } else if (connection.socket.readyState === OPEN) {
      clearTimer('foreground');
      checkHealth(source, true);
    } else {
      recover(connection, 'foreground_non_open_socket', true);
    }
  }

  function visibilityChange() {
    if (disposed) return;
    debug('visibility_change', {
      hidden: env.document.hidden,
      visibilityState: env.document.visibilityState,
    });
    if (env.document.hidden) {
      const clearedPendingPingRequestId = clearHealth();
      clearTimer('foreground');
      debug('backgrounded', { clearedPendingPingRequestId });
    } else {
      foreground('visibilitychange');
    }
  }
  const focus = () => foreground('focus');
  const pageShow = () => foreground('pageshow');

  return {
    start() {
      if (started || disposed) return;
      started = true;
      env.document.addEventListener('visibilitychange', visibilityChange);
      env.window.addEventListener('focus', focus);
      env.window.addEventListener('pageshow', pageShow);
      connect();
    },
    send: (message) => send(current, message),
    dispose() {
      if (disposed) return;
      debug('effect_cleanup');
      disposed = true;
      env.document.removeEventListener('visibilitychange', visibilityChange);
      env.window.removeEventListener('focus', focus);
      env.window.removeEventListener('pageshow', pageShow);
      for (const name of timers.keys()) clearTimer(name);
      const connection = current;
      current = null;
      if (connection) {
        for (const [name, listener] of Object.entries(connection.listeners))
          connection.socket.removeEventListener(name, listener);
        closeSocket(connection);
      }
    },
  };
}
