import { useCallback, useEffect, useRef, useState } from 'react';
import { reportClientErrorLater } from '../../lib/errorReporting.js';
import {
  BACKEND_WS_IDLE_PING_DELAY_MS,
  BACKEND_WS_INITIAL_RECONNECT_DELAY_MS,
  BACKEND_WS_MAX_RECONNECT_DELAY_MS,
  BACKEND_WS_PING_TIMEOUT_MS,
  DEFAULT_RADIO_STATE,
} from '../loggerScreenHelpers.js';
import {
  radioSocketUrl,
  wsjtxReceiptMessage,
  wsjtxTargetUpdate,
} from './radioSocketController.js';

const RADIO_WS_CONNECT_TIMEOUT_MS = 5000;

export function useRadioSocket({
  numericRadioId,
  numericLogId,
  loggerId,
  radioWebsocketUrl,
  notifyOperationalError,
  onWsjtXLoggedAdif,
}) {
  const [radioState, setRadioState] = useState(DEFAULT_RADIO_STATE);
  const [radioSocketStatus, setRadioSocketStatus] = useState('disconnected');
  const [catStatus, setCatStatus] = useState('offline');
  const [messageSentEvent, setMessageSentEvent] = useState(null);
  const [wsjtxTarget, setWsjtXTargetState] = useState(false);
  const radioSocketRef = useRef(null);
  const wsjtxTargetIntentRef = useRef(false);
  const onWsjtXLoggedAdifRef = useRef(onWsjtXLoggedAdif);
  onWsjtXLoggedAdifRef.current = onWsjtXLoggedAdif;

  const sendRadioMessage = useCallback((message) => {
    const socket = radioSocketRef.current;
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify(message));
    }
  }, []);

  const setWsjtXTarget = useCallback((enabled) => {
    const nextEnabled = Boolean(enabled);
    wsjtxTargetIntentRef.current = nextEnabled;
    setWsjtXTargetState(nextEnabled);
    const socket = radioSocketRef.current;
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(
        JSON.stringify({ type: 'set_wsjtx_target', enabled: nextEnabled }),
      );
    }
  }, []);

  useEffect(() => {
    setRadioState(DEFAULT_RADIO_STATE);
    setMessageSentEvent(null);
    setWsjtXTargetState(false);
    if (
      !radioWebsocketUrl ||
      !Number.isInteger(numericRadioId) ||
      numericRadioId <= 0 ||
      !Number.isInteger(numericLogId) ||
      numericLogId <= 0 ||
      !loggerId
    ) {
      setRadioSocketStatus('disconnected');
      setCatStatus('offline');
      return undefined;
    }

    let shouldReconnect = true;
    let reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
    let reconnectTimerId;
    let connectTimeoutTimerId;
    let healthCheckTimerId;
    let pongTimeoutTimerId;
    let lastMessageAt = Date.now();
    let pendingPingRequestId = null;

    function clearTimer(timerId) {
      if (timerId !== undefined) window.clearTimeout(timerId);
    }

    function clearHealthState() {
      clearTimer(healthCheckTimerId);
      clearTimer(pongTimeoutTimerId);
      healthCheckTimerId = undefined;
      pongTimeoutTimerId = undefined;
      pendingPingRequestId = null;
    }

    function scheduleReconnect() {
      if (!shouldReconnect || reconnectTimerId !== undefined) return;
      reconnectTimerId = window.setTimeout(() => {
        reconnectTimerId = undefined;
        connectRadioSocket();
      }, reconnectDelayMs);
      reconnectDelayMs = Math.min(
        reconnectDelayMs * 2,
        BACKEND_WS_MAX_RECONNECT_DELAY_MS,
      );
    }

    function scheduleHealthCheck() {
      clearTimer(healthCheckTimerId);
      healthCheckTimerId = undefined;
      const socket = radioSocketRef.current;
      if (
        !shouldReconnect ||
        document.hidden ||
        socket?.readyState !== WebSocket.OPEN ||
        pendingPingRequestId
      ) {
        return;
      }
      const idleMs = Date.now() - lastMessageAt;
      healthCheckTimerId = window.setTimeout(
        checkRadioSocketHealth,
        Math.max(BACKEND_WS_IDLE_PING_DELAY_MS - idleMs, 0),
      );
    }

    function markSocketActivity() {
      lastMessageAt = Date.now();
      pendingPingRequestId = null;
      clearTimer(pongTimeoutTimerId);
      pongTimeoutTimerId = undefined;
      scheduleHealthCheck();
    }

    function checkRadioSocketHealth({ forcePing = false } = {}) {
      healthCheckTimerId = undefined;
      if (!shouldReconnect || document.hidden) return;
      const socket = radioSocketRef.current;
      if (!socket || socket.readyState === WebSocket.CLOSED) {
        connectRadioSocket();
        return;
      }
      if (socket.readyState !== WebSocket.OPEN || pendingPingRequestId) {
        scheduleHealthCheck();
        return;
      }
      if (
        !forcePing &&
        Date.now() - lastMessageAt < BACKEND_WS_IDLE_PING_DELAY_MS
      ) {
        scheduleHealthCheck();
        return;
      }

      const requestId = window.crypto?.randomUUID
        ? window.crypto.randomUUID()
        : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
      pendingPingRequestId = requestId;
      try {
        socket.send(JSON.stringify({ type: 'ping', request_id: requestId }));
      } catch {
        pendingPingRequestId = null;
        socket.close();
        return;
      }
      pongTimeoutTimerId = window.setTimeout(() => {
        pongTimeoutTimerId = undefined;
        if (
          radioSocketRef.current === socket &&
          pendingPingRequestId === requestId
        ) {
          pendingPingRequestId = null;
          socket.close();
        }
      }, BACKEND_WS_PING_TIMEOUT_MS);
    }

    function connectRadioSocket() {
      if (!shouldReconnect) return;
      const current = radioSocketRef.current;
      if (
        current?.readyState === WebSocket.CONNECTING ||
        current?.readyState === WebSocket.OPEN
      ) {
        return;
      }

      clearTimer(reconnectTimerId);
      clearTimer(connectTimeoutTimerId);
      reconnectTimerId = undefined;
      connectTimeoutTimerId = undefined;
      clearHealthState();

      let url;
      try {
        url = radioSocketUrl(radioWebsocketUrl, {
          loggerId,
          logId: numericLogId,
        });
      } catch (error) {
        setRadioSocketStatus('disconnected');
        setCatStatus('offline');
        notifyOperationalError(
          'radioSocketUrl',
          'Unable to connect to radio I/O.',
          error,
          { radioId: numericRadioId, radioWebsocketUrl },
        );
        return;
      }

      setRadioSocketStatus('connecting');
      setCatStatus('offline');
      let socket;
      try {
        socket = new WebSocket(url);
      } catch (error) {
        setRadioSocketStatus('disconnected');
        notifyOperationalError(
          'radioSocketConnect',
          'Unable to connect to radio I/O.',
          error,
          { radioId: numericRadioId, url },
        );
        scheduleReconnect();
        return;
      }
      radioSocketRef.current = socket;
      connectTimeoutTimerId = window.setTimeout(() => {
        connectTimeoutTimerId = undefined;
        if (
          radioSocketRef.current === socket &&
          socket.readyState !== WebSocket.OPEN
        ) {
          socket.close();
        }
      }, RADIO_WS_CONNECT_TIMEOUT_MS);

      socket.addEventListener('open', () => {
        if (radioSocketRef.current !== socket) return;
        clearTimer(connectTimeoutTimerId);
        connectTimeoutTimerId = undefined;
        reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
        setRadioSocketStatus('connected');
        markSocketActivity();
        if (wsjtxTargetIntentRef.current) {
          socket.send(
            JSON.stringify({ type: 'set_wsjtx_target', enabled: true }),
          );
        }
      });

      socket.addEventListener('message', (event) => {
        if (radioSocketRef.current !== socket) return;
        markSocketActivity();
        try {
          const message = JSON.parse(event.data);
          if (message.type === 'radio_status') {
            setCatStatus(message.online ? 'online' : 'offline');
          } else if (message.type === 'radio_state') {
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
          } else if (message.type === 'wsjtx_target') {
            const targetUpdate = wsjtxTargetUpdate(message, {
              loggerId,
              logId: numericLogId,
              targetIntent: wsjtxTargetIntentRef.current,
            });
            setWsjtXTargetState(targetUpdate.isTarget);
            if (message.logger_id) {
              wsjtxTargetIntentRef.current = targetUpdate.isTarget;
            } else if (targetUpdate.reclaimTarget) {
              socket.send(
                JSON.stringify({ type: 'set_wsjtx_target', enabled: true }),
              );
            }
          } else if (message.type === 'wsjtx_logged_adif') {
            if (Number(message.log_id) !== numericLogId) {
              notifyOperationalError(
                'wsjtxWrongLog',
                'Ignored a WSJT-X contact for another log.',
                null,
                {
                  expectedLogId: numericLogId,
                  receivedLogId: message.log_id,
                  radioId: numericRadioId,
                },
              );
            } else {
              const accepted = onWsjtXLoggedAdifRef.current?.(message);
              const receipt = wsjtxReceiptMessage(
                message,
                numericLogId,
                accepted,
              );
              if (receipt) socket.send(JSON.stringify(receipt));
            }
          } else if (message.type === 'wsjtx_error') {
            notifyOperationalError(
              'wsjtx',
              'WSJT-X integration error.',
              message.message,
              { logId: numericLogId, radioId: numericRadioId },
            );
          }
        } catch (error) {
          if (messageIsWsjtXLoggedAdif(event.data)) {
            notifyOperationalError(
              'wsjtxLoggedAdif',
              'Unable to add the WSJT-X contact to the log.',
              error,
              { logId: numericLogId, radioId: numericRadioId },
            );
          }
          reportClientErrorLater({
            source: 'LoggerScreen.radioWebsocketMessage',
            message: 'Unable to process radio websocket message.',
            error,
            details: { radioId: numericRadioId },
          });
        }
      });

      socket.addEventListener('close', () => {
        if (radioSocketRef.current !== socket) return;
        radioSocketRef.current = null;
        clearTimer(connectTimeoutTimerId);
        connectTimeoutTimerId = undefined;
        clearHealthState();
        setRadioSocketStatus('disconnected');
        setCatStatus('offline');
        setWsjtXTargetState(false);
        scheduleReconnect();
      });

      socket.addEventListener('error', () => {
        if (radioSocketRef.current === socket) socket.close();
      });
    }

    function handleForeground() {
      if (document.hidden) return;
      const socket = radioSocketRef.current;
      if (socket?.readyState === WebSocket.OPEN) {
        checkRadioSocketHealth({ forcePing: true });
      } else {
        clearTimer(reconnectTimerId);
        reconnectTimerId = undefined;
        reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
        connectRadioSocket();
      }
    }

    function handleVisibilityChange() {
      if (document.hidden) {
        clearHealthState();
        return;
      }
      handleForeground();
    }

    document.addEventListener('visibilitychange', handleVisibilityChange);
    window.addEventListener('focus', handleForeground);
    window.addEventListener('pageshow', handleForeground);
    connectRadioSocket();
    return () => {
      shouldReconnect = false;
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      window.removeEventListener('focus', handleForeground);
      window.removeEventListener('pageshow', handleForeground);
      clearTimer(reconnectTimerId);
      clearTimer(connectTimeoutTimerId);
      clearHealthState();
      const socket = radioSocketRef.current;
      radioSocketRef.current = null;
      socket?.close();
      setCatStatus('offline');
    };
  }, [
    loggerId,
    numericLogId,
    numericRadioId,
    notifyOperationalError,
    radioWebsocketUrl,
  ]);

  return {
    radioState,
    radioSocketStatus,
    catStatus,
    messageSentEvent,
    sendRadioMessage,
    wsjtxTarget,
    setWsjtXTarget,
  };
}

function messageIsWsjtXLoggedAdif(raw) {
  try {
    return JSON.parse(raw)?.type === 'wsjtx_logged_adif';
  } catch {
    return false;
  }
}
