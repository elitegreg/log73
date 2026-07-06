import {
  startTransition,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import { websocketUrl } from '../../lib/api';
import { reportClientErrorLater } from '../../lib/errorReporting';
import {
  BACKEND_WS_IDLE_PING_DELAY_MS,
  BACKEND_WS_INITIAL_RECONNECT_DELAY_MS,
  BACKEND_WS_MAX_RECONNECT_DELAY_MS,
  BACKEND_WS_PING_TIMEOUT_MS,
  DEFAULT_RADIO_STATE,
  EMPTY_SCORE_SUMMARY,
} from '../loggerScreenHelpers.js';
import {
  formatSocketDebugDetails,
  readSocketDebugPanelEnabled,
  websocketReadyStateLabel,
} from './backendSocketController.js';

const FOREGROUND_CONNECTING_GRACE_MS = 3000;
const BACKEND_WS_CONNECT_TIMEOUT_MS = 5000;
const MAX_SOCKET_DEBUG_ENTRIES = 80;

export function useBackendSocket({
  sessionId,
  numericLogId,
  numericRadioId,
  notifyOperationalError,
  onSocketOpenRef,
  onSocketMessageRef,
  onRemoteContactRef,
  onRemoteContactDeletedRef,
  onRefreshContactsRef,
}) {
  const [radioState, setRadioState] = useState(DEFAULT_RADIO_STATE);
  const [backendSocketStatus, setBackendSocketStatus] =
    useState('disconnected');
  const [catStatus, setCatStatus] = useState('offline');
  const [messageSentEvent, setMessageSentEvent] = useState(null);
  const [scoreSummary, setScoreSummary] = useState(EMPTY_SCORE_SUMMARY);
  const [isSocketDebugPanelEnabled] = useState(readSocketDebugPanelEnabled);
  const [socketDebugEntries, setSocketDebugEntries] = useState([]);
  const backendSocketRef = useRef(null);
  const socketDebugSequenceRef = useRef(0);

  const sendRadioMessage = useCallback((message) => {
    const socket = backendSocketRef.current;
    if (socket?.readyState === WebSocket.OPEN)
      socket.send(JSON.stringify(message));
  }, []);

  useEffect(() => {
    let shouldReconnect = true;
    let reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
    let reconnectTimerId;
    let connectTimeoutTimerId;
    let foregroundReconnectTimerId;
    let healthCheckTimerId;
    let pongTimeoutTimerId;
    let lastMessageAt = Date.now();
    let pendingPingRequestId = null;
    const socketCreatedAtByInstance = new WeakMap();

    function socketStateLabel(socket = backendSocketRef.current) {
      if (!socket) return 'none';
      return websocketReadyStateLabel(socket.readyState);
    }

    function debugSocket(event, details = {}) {
      if (isSocketDebugPanelEnabled) {
        const entry = {
          id: socketDebugSequenceRef.current,
          timestamp: Date.now(),
          event,
          detailsText: formatSocketDebugDetails(details),
        };
        socketDebugSequenceRef.current += 1;
        startTransition(() => {
          setSocketDebugEntries((currentEntries) => {
            const nextEntries = [...currentEntries, entry];
            if (nextEntries.length <= MAX_SOCKET_DEBUG_ENTRIES)
              return nextEntries;
            return nextEntries.slice(-MAX_SOCKET_DEBUG_ENTRIES);
          });
        });
      }
      console.debug('[LoggerScreen websocket]', {
        event,
        sessionId,
        logId: numericLogId,
        radioId: numericRadioId,
        ...details,
      });
    }

    function clearReconnectTimer() {
      if (reconnectTimerId === undefined) return;
      window.clearTimeout(reconnectTimerId);
      reconnectTimerId = undefined;
    }

    function clearConnectTimeoutTimer() {
      if (connectTimeoutTimerId === undefined) return;
      window.clearTimeout(connectTimeoutTimerId);
      connectTimeoutTimerId = undefined;
    }

    function clearForegroundReconnectTimer() {
      if (foregroundReconnectTimerId === undefined) return;
      window.clearTimeout(foregroundReconnectTimerId);
      foregroundReconnectTimerId = undefined;
    }

    function clearHealthCheckTimer() {
      if (healthCheckTimerId === undefined) return;
      window.clearTimeout(healthCheckTimerId);
      healthCheckTimerId = undefined;
    }

    function clearPongTimeout() {
      if (pongTimeoutTimerId === undefined) return;
      window.clearTimeout(pongTimeoutTimerId);
      pongTimeoutTimerId = undefined;
    }

    function clearSocketHealthState() {
      const clearedPendingPingRequestId = pendingPingRequestId;
      pendingPingRequestId = null;
      clearHealthCheckTimer();
      clearPongTimeout();
      return clearedPendingPingRequestId;
    }

    function scheduleReconnect() {
      if (!shouldReconnect || reconnectTimerId !== undefined) return;
      const scheduledDelayMs = reconnectDelayMs;
      debugSocket('reconnect_scheduled', {
        delayMs: scheduledDelayMs,
        socketState: socketStateLabel(),
      });
      reconnectTimerId = window.setTimeout(() => {
        reconnectTimerId = undefined;
        debugSocket('reconnect_timer_fired', {
          socketState: socketStateLabel(),
        });
        connectBackendSocket();
      }, reconnectDelayMs);
      reconnectDelayMs = Math.min(
        reconnectDelayMs * 2,
        BACKEND_WS_MAX_RECONNECT_DELAY_MS,
      );
    }

    function scheduleHealthCheck() {
      if (!shouldReconnect || document.hidden) return;
      clearHealthCheckTimer();
      const socket = backendSocketRef.current;
      if (
        !socket ||
        socket.readyState !== WebSocket.OPEN ||
        pendingPingRequestId
      )
        return;
      const idleMs = Date.now() - lastMessageAt;
      const delayMs = Math.max(BACKEND_WS_IDLE_PING_DELAY_MS - idleMs, 0);
      healthCheckTimerId = window.setTimeout(() => {
        healthCheckTimerId = undefined;
        checkBackendSocketHealth();
      }, delayMs);
    }

    function markSocketActivity() {
      const clearedPendingPingRequestId = pendingPingRequestId;
      lastMessageAt = Date.now();
      pendingPingRequestId = null;
      clearPongTimeout();
      scheduleHealthCheck();
      return clearedPendingPingRequestId;
    }

    function reconnectBackendSocketNow(reason, details = {}) {
      const socket = backendSocketRef.current;
      const previousSocketState = socketStateLabel(socket);
      const clearedPendingPingRequestId = clearSocketHealthState();
      clearConnectTimeoutTimer();
      clearForegroundReconnectTimer();
      if (socket) backendSocketRef.current = null;
      debugSocket('reconnect_now', {
        reason,
        previousSocketState,
        clearedPendingPingRequestId,
        ...details,
      });
      setBackendSocketStatus('disconnected');
      setCatStatus('offline');
      socket?.close();
      clearReconnectTimer();
      reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
      connectBackendSocket();
    }

    function checkBackendSocketHealth({
      source = 'timer',
      forcePing = false,
    } = {}) {
      if (!shouldReconnect || document.hidden) {
        debugSocket('health_check_skipped', {
          source,
          forcePing,
          shouldReconnect,
          hidden: document.hidden,
          socketState: socketStateLabel(),
        });
        return;
      }
      const socket = backendSocketRef.current;
      const idleMs = Date.now() - lastMessageAt;
      debugSocket('health_check_started', {
        source,
        forcePing,
        idleMs,
        socketState: socketStateLabel(socket),
        pendingPingRequestId,
      });
      if (!socket || socket.readyState === WebSocket.CLOSED) {
        clearReconnectTimer();
        reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
        debugSocket('health_check_reconnect_now', {
          source,
          reason: socket ? 'closed_socket' : 'missing_socket',
        });
        connectBackendSocket();
        return;
      }
      if (socket.readyState === WebSocket.CONNECTING || pendingPingRequestId) {
        debugSocket('health_check_waiting', {
          source,
          forcePing,
          socketState: socketStateLabel(socket),
          pendingPingRequestId,
        });
        scheduleHealthCheck();
        return;
      }
      if (socket.readyState !== WebSocket.OPEN) {
        debugSocket('health_check_closing_socket', {
          source,
          forcePing,
          socketState: socketStateLabel(socket),
        });
        socket.close();
        return;
      }
      if (!forcePing && idleMs < BACKEND_WS_IDLE_PING_DELAY_MS) {
        debugSocket('health_check_recent_activity', {
          source,
          idleMs,
          thresholdMs: BACKEND_WS_IDLE_PING_DELAY_MS,
        });
        scheduleHealthCheck();
        return;
      }

      const requestId = window.crypto?.randomUUID
        ? window.crypto.randomUUID()
        : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
      pendingPingRequestId = requestId;
      debugSocket('ping_sent', {
        source,
        forcePing,
        requestId,
        idleMs,
        timeoutMs: BACKEND_WS_PING_TIMEOUT_MS,
      });
      try {
        socket.send(JSON.stringify({ type: 'ping', request_id: requestId }));
      } catch (error) {
        pendingPingRequestId = null;
        debugSocket('ping_send_failed', {
          source,
          forcePing,
          requestId,
          socketState: socketStateLabel(socket),
          error:
            error instanceof Error
              ? { name: error.name, message: error.message }
              : String(error),
        });
        reconnectBackendSocketNow('ping_send_failed', {
          source,
          forcePing,
          requestId,
        });
        return;
      }
      clearPongTimeout();
      pongTimeoutTimerId = window.setTimeout(() => {
        pongTimeoutTimerId = undefined;
        if (
          backendSocketRef.current === socket &&
          pendingPingRequestId === requestId
        ) {
          pendingPingRequestId = null;
          debugSocket('pong_timeout', {
            source,
            forcePing,
            requestId,
            timeoutMs: BACKEND_WS_PING_TIMEOUT_MS,
            socketState: socketStateLabel(socket),
          });
          reconnectBackendSocketNow('pong_timeout', {
            source,
            forcePing,
            requestId,
          });
        }
      }, BACKEND_WS_PING_TIMEOUT_MS);
    }

    function handleForeground(source) {
      if (document.hidden) {
        debugSocket('foreground_ignored_hidden', {
          source,
          visibilityState: document.visibilityState,
          socketState: socketStateLabel(),
        });
        return;
      }
      const socket = backendSocketRef.current;
      debugSocket('foreground_event', {
        source,
        visibilityState: document.visibilityState,
        socketState: socketStateLabel(socket),
        idleMs: Date.now() - lastMessageAt,
      });
      if (!socket || socket.readyState === WebSocket.CLOSED) {
        clearForegroundReconnectTimer();
        clearReconnectTimer();
        reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
        debugSocket('foreground_reconnect_now', {
          source,
          reason: socket ? 'closed_socket' : 'missing_socket',
        });
        connectBackendSocket();
        return;
      }
      if (socket.readyState === WebSocket.CONNECTING) {
        const socketAgeMs = Math.max(
          Date.now() - (socketCreatedAtByInstance.get(socket) ?? Date.now()),
          0,
        );
        if (socketAgeMs < FOREGROUND_CONNECTING_GRACE_MS) {
          clearForegroundReconnectTimer();
          const graceRemainingMs = FOREGROUND_CONNECTING_GRACE_MS - socketAgeMs;
          debugSocket('foreground_connecting_grace', {
            source,
            socketState: socketStateLabel(socket),
            socketAgeMs,
            graceRemainingMs,
          });
          foregroundReconnectTimerId = window.setTimeout(() => {
            foregroundReconnectTimerId = undefined;
            if (backendSocketRef.current !== socket) return;
            handleForeground(`${source}:connecting_grace_elapsed`);
          }, graceRemainingMs);
          return;
        }
        reconnectBackendSocketNow('foreground_connecting_stalled', {
          source,
          socketState: socketStateLabel(socket),
          socketAgeMs,
          graceMs: FOREGROUND_CONNECTING_GRACE_MS,
        });
        return;
      }
      if (socket.readyState !== WebSocket.OPEN) {
        reconnectBackendSocketNow('foreground_non_open_socket', {
          source,
          socketState: socketStateLabel(socket),
        });
        return;
      }
      clearForegroundReconnectTimer();
      if (socket.readyState === WebSocket.OPEN) {
        checkBackendSocketHealth({ source, forcePing: true });
      }
    }

    function handleVisibilityChange() {
      debugSocket('visibility_change', {
        hidden: document.hidden,
        visibilityState: document.visibilityState,
        socketState: socketStateLabel(),
        pendingPingRequestId,
      });
      if (document.hidden) {
        const clearedPendingPingRequestId = clearSocketHealthState();
        clearForegroundReconnectTimer();
        debugSocket('backgrounded', {
          visibilityState: document.visibilityState,
          clearedPendingPingRequestId,
        });
        return;
      }
      handleForeground('visibilitychange');
    }

    function handleFocus() {
      handleForeground('focus');
    }

    function handlePageShow() {
      handleForeground('pageshow');
    }

    function connectBackendSocket() {
      if (!shouldReconnect) return;
      const existingSocket = backendSocketRef.current;
      if (
        existingSocket &&
        (existingSocket.readyState === WebSocket.CONNECTING ||
          existingSocket.readyState === WebSocket.OPEN)
      ) {
        debugSocket('connect_skipped_existing_socket', {
          socketState: socketStateLabel(existingSocket),
        });
        return;
      }
      clearReconnectTimer();
      clearConnectTimeoutTimer();
      const clearedPendingPingRequestId = clearSocketHealthState();
      const url = websocketUrl({
        session_id: sessionId,
        log_id: numericLogId,
        radio_id: numericRadioId,
      });
      debugSocket('connect_attempt', {
        url,
        clearedPendingPingRequestId,
      });
      setBackendSocketStatus('connecting');
      setCatStatus('offline');
      const socket = new WebSocket(url);
      socketCreatedAtByInstance.set(socket, Date.now());
      backendSocketRef.current = socket;
      connectTimeoutTimerId = window.setTimeout(() => {
        connectTimeoutTimerId = undefined;
        if (backendSocketRef.current !== socket) return;
        if (socket.readyState === WebSocket.OPEN) return;
        debugSocket('connect_timeout', {
          socketState: socketStateLabel(socket),
          timeoutMs: BACKEND_WS_CONNECT_TIMEOUT_MS,
        });
        reconnectBackendSocketNow('connect_timeout', {
          socketState: socketStateLabel(socket),
          timeoutMs: BACKEND_WS_CONNECT_TIMEOUT_MS,
        });
      }, BACKEND_WS_CONNECT_TIMEOUT_MS);
      socket.addEventListener('open', async () => {
        if (backendSocketRef.current !== socket) return;
        clearConnectTimeoutTimer();
        clearForegroundReconnectTimer();
        reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
        setBackendSocketStatus('connected');
        const clearedPendingPingRequestId = markSocketActivity();
        debugSocket('socket_open', {
          socketState: socketStateLabel(socket),
          clearedPendingPingRequestId,
        });
        onRefreshContactsRef.current?.();
        await onSocketOpenRef.current?.();
      });
      socket.addEventListener('message', (event) => {
        if (backendSocketRef.current !== socket) return;
        const clearedPendingPingRequestId = markSocketActivity();
        try {
          const message = JSON.parse(event.data);
          if (message.type === 'pong') {
            debugSocket('pong_received', {
              requestId: message.request_id,
              clearedPendingPingRequestId,
              socketState: socketStateLabel(socket),
            });
          } else if (clearedPendingPingRequestId) {
            debugSocket('socket_activity_cleared_ping', {
              messageType: message.type,
              clearedPendingPingRequestId,
              socketState: socketStateLabel(socket),
            });
          }
          if (message.type === 'radio_status') {
            setCatStatus(message.online ? 'online' : 'offline');
          } else if (message.type === 'radio_state') {
            debugSocket('radio_state_received', {
              frequencyHz: message.frequency_hz,
              mode: message.mode,
              ritOffsetHz: message.rit_offset_hz,
              socketState: socketStateLabel(socket),
            });
            setRadioState({
              frequency_hz: message.frequency_hz,
              mode: message.mode,
              rit_offset_hz: Number(message.rit_offset_hz ?? 0),
            });
          } else if (message.type === 'message_sent') {
            setMessageSentEvent({
              requestId: message.request_id,
              sequence: Date.now(),
            });
          } else if (message.type === 'log_entry') {
            onRemoteContactRef.current?.(message.contact);
          } else if (message.type === 'contact_deleted') {
            onRemoteContactDeletedRef.current?.(message.id);
          } else if (message.type === 'score_update') {
            setScoreSummary({
              qsoCount: Number(message.qso_count ?? 0),
              multipliers: Number(message.multipliers ?? 0),
              bonusPoints: Number(message.bonus_points ?? 0),
              score: Number(message.total_score ?? 0),
            });
          }
          onSocketMessageRef.current?.(message);
        } catch (error) {
          reportClientErrorLater({
            source: 'LoggerScreen.websocketMessage',
            message: 'Unable to process backend websocket message.',
            error,
            details: { logId: numericLogId, radioId: numericRadioId },
          });
          debugSocket('message_parse_failed', {
            socketState: socketStateLabel(socket),
          });
        }
      });
      socket.addEventListener('close', (event) => {
        if (backendSocketRef.current === socket) {
          backendSocketRef.current = null;
          clearConnectTimeoutTimer();
          clearForegroundReconnectTimer();
          const clearedPendingPingRequestId = clearSocketHealthState();
          debugSocket('socket_close', {
            code: event.code,
            reason: event.reason,
            wasClean: event.wasClean,
            clearedPendingPingRequestId,
          });
          setBackendSocketStatus('disconnected');
          setCatStatus('offline');
          scheduleReconnect();
        }
      });
      socket.addEventListener('error', () => {
        if (backendSocketRef.current !== socket) return;
        clearConnectTimeoutTimer();
        clearForegroundReconnectTimer();
        const clearedPendingPingRequestId = clearSocketHealthState();
        debugSocket('socket_error', {
          socketState: socketStateLabel(socket),
          clearedPendingPingRequestId,
        });
        setBackendSocketStatus('disconnected');
        setCatStatus('offline');
        socket.close();
      });
    }

    document.addEventListener('visibilitychange', handleVisibilityChange);
    window.addEventListener('focus', handleFocus);
    window.addEventListener('pageshow', handlePageShow);
    connectBackendSocket();
    return () => {
      shouldReconnect = false;
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      window.removeEventListener('focus', handleFocus);
      window.removeEventListener('pageshow', handlePageShow);
      clearReconnectTimer();
      clearConnectTimeoutTimer();
      clearForegroundReconnectTimer();
      const clearedPendingPingRequestId = clearSocketHealthState();
      debugSocket('effect_cleanup', {
        clearedPendingPingRequestId,
        socketState: socketStateLabel(),
      });
      const socket = backendSocketRef.current;
      backendSocketRef.current = null;
      setCatStatus('offline');
      socket?.close();
    };
  }, [
    isSocketDebugPanelEnabled,
    notifyOperationalError,
    numericLogId,
    numericRadioId,
    onRefreshContactsRef,
    onRemoteContactDeletedRef,
    onRemoteContactRef,
    onSocketMessageRef,
    onSocketOpenRef,
    sessionId,
  ]);

  useEffect(() => {
    setScoreSummary(EMPTY_SCORE_SUMMARY);
  }, [numericLogId]);

  return {
    radioState,
    backendSocketStatus,
    catStatus,
    messageSentEvent,
    scoreSummary,
    isSocketDebugPanelEnabled,
    socketDebugEntries,
    sendRadioMessage,
  };
}
