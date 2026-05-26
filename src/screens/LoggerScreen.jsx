import React, {
  startTransition,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { apiJson, websocketUrl } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';
import LogWindow from '../logger/LogWindow';
import MainWindow from '../logger/MainWindow';
import {
  BACKEND_WS_IDLE_PING_DELAY_MS,
  BACKEND_WS_INITIAL_RECONNECT_DELAY_MS,
  BACKEND_WS_MAX_RECONNECT_DELAY_MS,
  BACKEND_WS_PING_TIMEOUT_MS,
  CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS,
  CONTACTS_LOAD_MAX_RETRY_DELAY_MS,
  CONTACTS_PAGE_SIZE,
  CONTACT_COMMIT_RETRY_DELAY_MS,
  DEFAULT_RADIO_STATE,
  EMPTY_SCORE_SUMMARY,
  getSessionId,
  loadLocalContacts,
  saveLocalContacts,
  committedBackendContact,
  mergeContact,
  sortContacts,
  markContactFailed,
  contactIdentifier,
} from './loggerScreenHelpers';

let promptedOperatorCallsign;
const SOCKET_READY_STATE_LABELS = ['connecting', 'open', 'closing', 'closed'];
const FOREGROUND_CONNECTING_GRACE_MS = 1000;
const BACKEND_WS_CONNECT_TIMEOUT_MS = 2000;
const SOCKET_DEBUG_PANEL_QUERY_PARAM = 'socket_debug';
const SOCKET_DEBUG_PANEL_STORAGE_KEY = 'log73.socket_debug_panel';
const MAX_SOCKET_DEBUG_ENTRIES = 80;
const MAX_SOCKET_DEBUG_DETAILS_LENGTH = 240;

function promptForOperatorCallsign(defaultCallsign) {
  const enteredCallsign = window.prompt(
    'Operator Callsign',
    promptedOperatorCallsign ?? defaultCallsign,
  );
  if (enteredCallsign === null)
    return promptedOperatorCallsign ?? defaultCallsign;
  promptedOperatorCallsign = enteredCallsign.toUpperCase();
  return promptedOperatorCallsign;
}

function websocketReadyStateLabel(readyState) {
  return SOCKET_READY_STATE_LABELS[readyState] ?? `unknown(${readyState})`;
}

function formatSocketDebugTimestamp(timestamp) {
  return new Date(timestamp).toISOString().slice(11, 23);
}

function formatSocketDebugDetails(details) {
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

// Runtime toggle for the on-screen socket log panel used during iPad debugging.
function readSocketDebugPanelEnabled() {
  if (typeof window === 'undefined') return false;

  try {
    const params = new URLSearchParams(window.location.search);
    const queryValue = params.get(SOCKET_DEBUG_PANEL_QUERY_PARAM);
    if (queryValue === '1') {
      window.localStorage?.setItem(SOCKET_DEBUG_PANEL_STORAGE_KEY, '1');
      return true;
    }
    if (queryValue === '0') {
      window.localStorage?.removeItem(SOCKET_DEBUG_PANEL_STORAGE_KEY);
      return false;
    }
    return window.localStorage?.getItem(SOCKET_DEBUG_PANEL_STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

function LoggerScreen() {
  const { logId, radioId } = useParams();
  const navigate = useNavigate();
  const { notifyError } = useNotifications();
  const numericLogId = Number(logId);
  const numericRadioId = Number(radioId);
  const [settings, setSettings] = useState(null);
  const [log, setLog] = useState(null);
  const [radio, setRadio] = useState(null);
  const [cwLabels, setCwLabels] = useState(null);
  const [cwSentEvent, setCwSentEvent] = useState(null);
  const [contacts, setContacts] = useState(() => loadLocalContacts(logId));
  const [operatorCallsign, setOperatorCallsign] = useState('');
  const [sessionId] = useState(getSessionId);
  const [radioState, setRadioState] = useState(DEFAULT_RADIO_STATE);
  const [backendSocketStatus, setBackendSocketStatus] =
    useState('disconnected');
  const [catStatus, setCatStatus] = useState('offline');
  const [scoreSummary, setScoreSummary] = useState(EMPTY_SCORE_SUMMARY);
  const [isContextLoading, setIsContextLoading] = useState(true);
  const [contactsLoadState, setContactsLoadState] = useState('initial-loading');
  const [isRescoreLoading, setIsRescoreLoading] = useState(false);
  const [isSocketDebugPanelEnabled] = useState(readSocketDebugPanelEnabled);
  const [socketDebugEntries, setSocketDebugEntries] = useState([]);
  const backendSocketRef = useRef(null);
  const committingContactIdsRef = useRef(new Set());
  const refreshContactsRef = useRef(() => {});
  const contactsLoadErrorNotifiedRef = useRef(false);
  const commitContactErrorNotifiedRef = useRef(false);
  const socketDebugSequenceRef = useRef(0);

  const notifyOperationalError = useCallback(
    (source, fallback, error, details = {}) => {
      const message = errorMessage(error, fallback);
      notifyError(message, { dedupeKey: `${source}:${message}` });
      reportClientErrorLater({
        source,
        message,
        error,
        details,
      });
    },
    [notifyError],
  );

  useEffect(() => {
    saveLocalContacts(logId, contacts);
  }, [contacts, logId]);

  useEffect(() => {
    setScoreSummary(EMPTY_SCORE_SUMMARY);
  }, [numericLogId]);

  useEffect(() => {
    let isCancelled = false;

    async function loadContext() {
      setIsContextLoading(true);
      const [logResult, radioResult, cwLabelsResult] = await Promise.all([
        apiJson(`/logs/${numericLogId}`),
        apiJson(`/radios/${numericRadioId}`),
        apiJson(`/radios/${numericRadioId}/cw-labels`),
      ]);
      if (!logResult.ok) throw new Error(logResult.error ?? 'Log not found');
      if (!radioResult.ok)
        throw new Error(radioResult.error ?? 'Radio not found');
      const contestSettings = await apiJson(
        `/contest-settings?contest_id=${encodeURIComponent(logResult.log.contest_id)}`,
      );
      if (isCancelled) return;
      setSettings(contestSettings);
      setLog(logResult.log);
      setRadio(radioResult.radio);
      if (cwLabelsResult.ok) setCwLabels(cwLabelsResult.labels);
      setOperatorCallsign(
        (current) =>
          current || promptForOperatorCallsign(logResult.log.station_callsign),
      );
    }
    const loadContextPromise = loadContext();
    loadContextPromise.catch((error) =>
      notifyOperationalError(
        'LoggerScreen.loadContext',
        'Unable to load logger context.',
        error,
        { logId: numericLogId, radioId: numericRadioId },
      ),
    );
    loadContextPromise.finally(() => {
      if (!isCancelled) setIsContextLoading(false);
    });
    return () => {
      isCancelled = true;
    };
  }, [numericLogId, numericRadioId, notifyOperationalError]);

  useEffect(() => {
    function handleKeyDown(event) {
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key.toLowerCase() === 'o'
      ) {
        event.preventDefault();
        setOperatorCallsign(
          promptForOperatorCallsign(log?.station_callsign ?? ''),
        );
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [log]);

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
      socket.addEventListener('open', () => {
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
        refreshContactsRef.current();
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
            setRadioState({
              frequency_hz: message.frequency_hz,
              mode: message.mode,
            });
          } else if (message.type === 'cw_sent') {
            setCwSentEvent({
              requestId: message.request_id,
              sequence: Date.now(),
            });
          } else if (
            message.type === 'log_entry' &&
            message.contact?._session_id !== sessionId &&
            Number(message.contact?._log_id) === numericLogId
          ) {
            setContacts((currentContacts) =>
              mergeContact(currentContacts, message.contact),
            );
          } else if (
            message.type === 'contact_deleted' &&
            Number(message.log_id) === numericLogId
          ) {
            setContacts((currentContacts) =>
              currentContacts.filter(
                (contact) => String(contact._id) !== String(message.id),
              ),
            );
          } else if (
            message.type === 'score_update' &&
            Number(message.log_id) === numericLogId
          ) {
            setScoreSummary({
              qsoCount: Number(message.qso_count ?? 0),
              multipliers: Number(message.multipliers ?? 0),
              bonusPoints: Number(message.bonus_points ?? 0),
              score: Number(message.total_score ?? 0),
            });
          } else if (message.type === 'pong') {
            // Any inbound message proves the socket is still healthy.
          }
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
  }, [sessionId, numericLogId, numericRadioId, isSocketDebugPanelEnabled]);

  useEffect(() => {
    let shouldRetryContactsLoad = true;
    let contactsLoadRetryDelayMs = CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS;
    let contactsLoadRetryTimerId;
    let contactsLoadInFlightPromise = null;

    function scheduleContactsLoadRetry() {
      if (!shouldRetryContactsLoad) return false;
      if (contactsLoadRetryTimerId !== undefined) return true;
      contactsLoadRetryTimerId = window.setTimeout(() => {
        contactsLoadRetryTimerId = undefined;
        loadContacts({ mode: 'retry' });
      }, contactsLoadRetryDelayMs);
      contactsLoadRetryDelayMs = Math.min(
        contactsLoadRetryDelayMs * 2,
        CONTACTS_LOAD_MAX_RETRY_DELAY_MS,
      );
      return true;
    }

    function loadContacts({ mode = 'refresh' } = {}) {
      if (contactsLoadInFlightPromise) return contactsLoadInFlightPromise;
      setContactsLoadState((currentState) => {
        if (mode === 'retry') return 'retrying';
        if (mode === 'initial') return 'initial-loading';
        if (currentState === 'initial-loading') return 'initial-loading';
        return 'refreshing';
      });

      contactsLoadInFlightPromise = (async () => {
        try {
          const backendContacts = [];
          let offset = 0;

          while (true) {
            const page = await apiJson(
              `/logs/${numericLogId}/contacts?limit=${CONTACTS_PAGE_SIZE}&offset=${offset}`,
            );
            const committedPage = page.map(committedBackendContact);
            backendContacts.push(...committedPage);
            if (committedPage.length < CONTACTS_PAGE_SIZE) {
              break;
            }
            offset += CONTACTS_PAGE_SIZE;
          }

          const localUncommittedContacts = loadLocalContacts(logId).filter(
            (contact) => contact._status !== 'Committed',
          );
          setContacts(
            sortContacts([...backendContacts, ...localUncommittedContacts]),
          );
          shouldRetryContactsLoad = false;
          contactsLoadErrorNotifiedRef.current = false;
          setContactsLoadState('idle');
          return true;
        } catch (error) {
          if (!contactsLoadErrorNotifiedRef.current) {
            contactsLoadErrorNotifiedRef.current = true;
            notifyOperationalError(
              'LoggerScreen.loadContacts',
              'Unable to load backend contacts. Using local contacts and retrying.',
              error,
              { logId: numericLogId },
            );
          }
          const retryScheduled = scheduleContactsLoadRetry();
          setContactsLoadState(retryScheduled ? 'retrying' : 'idle');
          return false;
        } finally {
          contactsLoadInFlightPromise = null;
        }
      })();

      return contactsLoadInFlightPromise;
    }

    refreshContactsRef.current = loadContacts;
    setContactsLoadState('initial-loading');
    loadContacts({ mode: 'initial' });
    return () => {
      refreshContactsRef.current = () => {};
      shouldRetryContactsLoad = false;
      if (contactsLoadRetryTimerId !== undefined)
        window.clearTimeout(contactsLoadRetryTimerId);
    };
  }, [numericLogId, logId, notifyOperationalError]);

  useEffect(() => {
    const pendingContact = contacts.find((contact) => {
      if (contact._status === 'Pending')
        return (
          contact._client_id &&
          !committingContactIdsRef.current.has(contact._client_id)
        );
      if (contact._status === 'Updating') {
        const updateKey = contact._id ?? contact._client_id;
        return updateKey && !committingContactIdsRef.current.has(updateKey);
      }
      return false;
    });
    if (!pendingContact) return;

    const commitKey =
      pendingContact._status === 'Pending'
        ? pendingContact._client_id
        : (pendingContact._id ?? pendingContact._client_id);
    committingContactIdsRef.current.add(commitKey);

    async function commitContact(contact) {
      try {
        const responseBody = await apiJson(`/logs/${numericLogId}/contacts`, {
          method: 'POST',
          body: JSON.stringify({ ...contact, _log_id: numericLogId }),
        });
        if (!responseBody.ok) {
          notifyOperationalError(
            'LoggerScreen.commitContactRejected',
            responseBody.error ?? 'Contact upload failed.',
            responseBody.error,
            {
              logId: numericLogId,
              contactId: contact._id ?? contact._client_id ?? null,
            },
          );
          setContacts((currentContacts) =>
            markContactFailed(
              currentContacts,
              contact,
              responseBody.error ?? 'Contact upload failed.',
            ),
          );
          return;
        }
        if (responseBody.contact) {
          commitContactErrorNotifiedRef.current = false;
          setContacts((currentContacts) =>
            mergeContact(currentContacts, {
              ...responseBody.contact,
              _client_id: contact._client_id,
            }),
          );
        } else {
          notifyOperationalError(
            'LoggerScreen.commitContactMissing',
            'Contact upload failed: server response did not include a committed contact.',
            null,
            {
              logId: numericLogId,
              contactId: contact._id ?? contact._client_id ?? null,
            },
          );
          setContacts((currentContacts) =>
            markContactFailed(
              currentContacts,
              contact,
              'Contact upload failed: server response did not include a committed contact.',
            ),
          );
        }
      } catch (error) {
        if (!commitContactErrorNotifiedRef.current) {
          commitContactErrorNotifiedRef.current = true;
          notifyOperationalError(
            'LoggerScreen.commitContactRetry',
            'Unable to commit contact. Retrying.',
            error,
            {
              logId: numericLogId,
              contactId: contact._id ?? contact._client_id ?? null,
            },
          );
        }
        window.setTimeout(
          () => setContacts((currentContacts) => sortContacts(currentContacts)),
          CONTACT_COMMIT_RETRY_DELAY_MS,
        );
      } finally {
        committingContactIdsRef.current.delete(commitKey);
      }
    }

    commitContact(pendingContact);
  }, [contacts, numericLogId, notifyOperationalError]);

  function sendRadioMessage(message) {
    const socket = backendSocketRef.current;
    if (socket?.readyState === WebSocket.OPEN)
      socket.send(JSON.stringify(message));
  }

  async function handleRescore() {
    if (isRescoreLoading || contactsLoadState !== 'idle') return;
    setIsRescoreLoading(true);
    try {
      await refreshContactsRef.current({ mode: 'refresh' });
    } finally {
      setIsRescoreLoading(false);
    }
  }

  async function deleteContacts(contactsToDelete) {
    if (contactsToDelete.length === 0) return;

    const qsoLabel =
      contactsToDelete.length === 1
        ? '1 QSO'
        : `${contactsToDelete.length} QSOs`;
    if (!window.confirm(`Are you sure you want to delete ${qsoLabel}?`)) return;

    const committedContacts = contactsToDelete.filter(
      (contact) => contact._id !== undefined,
    );
    const localContactIdentifiers = contactsToDelete
      .filter((contact) => contact._id === undefined)
      .map(contactIdentifier)
      .filter(Boolean);
    const successfullyDeletedIds = [];
    const results = await Promise.allSettled(
      committedContacts.map(async (contact) => {
        const result = await apiJson(`/contacts/${contact._id}`, {
          method: 'DELETE',
        });
        if (!result.ok)
          throw new Error(result.error ?? 'Unable to delete contact');
        if (result.deleted) successfullyDeletedIds.push(String(contact._id));
      }),
    );
    const failureCount = results.filter(
      (result) => result.status === 'rejected',
    ).length;
    const deleteFailures = results
      .map((result, index) => {
        if (result.status !== 'rejected') return null;
        return {
          id: committedContacts[index]?._id ?? null,
          error: errorMessage(result.reason, 'Unable to delete contact'),
        };
      })
      .filter(Boolean);
    const deletedIdentifiers = new Set([
      ...successfullyDeletedIds.map((id) => `id:${id}`),
      ...localContactIdentifiers,
    ]);

    setContacts((currentContacts) =>
      currentContacts.filter((contact) => {
        const identifier = contactIdentifier(contact);
        return !identifier || !deletedIdentifiers.has(identifier);
      }),
    );

    if (failureCount > 0) {
      const message = `Unable to delete ${failureCount === 1 ? '1 QSO' : `${failureCount} QSOs`}.`;
      notifyError(message, {
        dedupeKey: `LoggerScreen.deleteContacts:${failureCount}`,
      });
      reportClientErrorLater({
        source: 'LoggerScreen.deleteContacts',
        message,
        details: {
          logId: numericLogId,
          failures: deleteFailures,
        },
      });
    }
  }

  function updateContacts(contactsToUpdate, field, value) {
    const identifiers = new Set(
      contactsToUpdate.map(contactIdentifier).filter(Boolean),
    );
    if (identifiers.size === 0) return;

    setContacts((currentContacts) =>
      sortContacts(
        currentContacts.map((contact) => {
          const identifier = contactIdentifier(contact);
          if (!identifier || !identifiers.has(identifier)) return contact;
          return {
            ...contact,
            [field]: value,
            _status: contact._id === undefined ? 'Pending' : 'Updating',
            _error: undefined,
          };
        }),
      ),
    );
  }

  function exitLogger() {
    navigate('/ui/open_log');
  }

  return (
    <div className="app-container">
      <MainWindow
        settings={settings}
        log={log}
        radio={radio}
        isContextLoading={isContextLoading}
        contactsLoadState={contactsLoadState}
        stationCallsign={log?.station_callsign ?? ''}
        operatorCallsign={operatorCallsign}
        radioState={radioState}
        backendSocketStatus={backendSocketStatus}
        catStatus={catStatus}
        cwLabels={cwLabels}
        cwSentEvent={cwSentEvent}
        sessionId={sessionId}
        logId={numericLogId}
        onSetRadioFrequency={(frequencyHz) =>
          sendRadioMessage({ type: 'set_frequency', frequency_hz: frequencyHz })
        }
        onSetRadioMode={(mode) => sendRadioMessage({ type: 'set_mode', mode })}
        onSendCw={(payload) =>
          sendRadioMessage({ type: 'send_cw', ...payload })
        }
        onStopCw={() => sendRadioMessage({ type: 'stop_cw' })}
        onSetCwWpm={(wpm) => sendRadioMessage({ type: 'set_wpm', wpm })}
        onLogContact={(contact) =>
          setContacts((currentContacts) =>
            sortContacts([...currentContacts, contact]),
          )
        }
        onRescore={handleRescore}
        isRescoreLoading={isRescoreLoading}
        scoreSummary={scoreSummary}
        onExit={exitLogger}
      />
      <LogWindow
        settings={settings}
        contacts={contacts}
        log={log}
        contactsLoadState={contactsLoadState}
        radioMode={radioState?.mode ?? 'CW'}
        onDeleteContacts={deleteContacts}
        onUpdateContacts={updateContacts}
      />
      {isSocketDebugPanelEnabled && (
        <div
          style={{
            width: '100%',
            maxWidth: '1600px',
            marginTop: '8px',
            border: '1px solid #808080',
            backgroundColor: '#f7f7f7',
            color: '#111',
            fontFamily: 'monospace',
            fontSize: '11px',
            lineHeight: '1.35',
          }}
        >
          <div
            style={{
              padding: '4px 6px',
              borderBottom: '1px solid #c0c0c0',
              backgroundColor: '#e8e8e8',
              fontWeight: 'bold',
            }}
          >
            Logger websocket debug
          </div>
          <div
            style={{
              maxHeight: '180px',
              overflowY: 'auto',
              padding: '4px 6px',
            }}
          >
            {socketDebugEntries.length === 0 ? (
              <div>Waiting for websocket events...</div>
            ) : (
              socketDebugEntries.map((entry) => (
                <div
                  key={entry.id}
                  style={{
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-word',
                    borderBottom: '1px dotted #d0d0d0',
                    padding: '1px 0',
                  }}
                >
                  {formatSocketDebugTimestamp(entry.timestamp)} {entry.event}
                  {entry.detailsText ? ` ${entry.detailsText}` : ''}
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export default LoggerScreen;
