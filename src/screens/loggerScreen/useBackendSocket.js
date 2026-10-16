import {
  startTransition,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { websocketUrl } from '../../lib/api';
import { reportClientErrorLater } from '../../lib/errorReporting';
import { EMPTY_SCORE_SUMMARY } from '../loggerScreenHelpers.js';
import {
  formatSocketDebugDetails,
  logSocketDebug,
  readSocketDebugPanelEnabled,
} from './backendSocketController.js';
import { createReconnectingSocket } from './reconnectingSocketController.js';

const MAX_SOCKET_DEBUG_ENTRIES = 80;

export function useBackendSocket({
  sessionId,
  loggerId,
  numericLogId,
  numericRadioId,
  onSocketOpenRef,
  onSocketMessageRef,
  onRemoteContactRef,
  onRemoteContactDeletedRef,
  onRefreshContactsRef,
}) {
  const [backendSocketStatus, setBackendSocketStatus] =
    useState('disconnected');
  const [scoreSummary, setScoreSummary] = useState(EMPTY_SCORE_SUMMARY);
  const [isSocketDebugPanelEnabled] = useState(readSocketDebugPanelEnabled);
  const [socketDebugEntries, setSocketDebugEntries] = useState([]);
  const controllerRef = useRef(null);
  const callbacksRef = useRef({});
  const socketDebugSequenceRef = useRef(0);

  const sendBackendMessage = useCallback((message) => {
    controllerRef.current?.send(message);
  }, []);

  // Publish callbacks only after commit. Updating them does not replace a socket.
  useLayoutEffect(() => {
    callbacksRef.current = {
      onStatusChange: setBackendSocketStatus,
      async onOpen({ isCurrent }) {
        onRefreshContactsRef.current?.();
        if (isCurrent()) await onSocketOpenRef.current?.();
      },
      onMessage(message, { isCurrent }) {
        if (message.type === 'log_entry') {
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
        if (isCurrent()) onSocketMessageRef.current?.(message);
      },
      onMessageError(error) {
        reportClientErrorLater({
          source: 'LoggerScreen.websocketMessage',
          message: 'Unable to process backend websocket message.',
          error,
          details: { logId: numericLogId, radioId: numericRadioId },
        });
      },
      onError(error, { phase }) {
        if (phase === 'socket') return;
        reportClientErrorLater({
          source: 'LoggerScreen.websocketConnection',
          message: 'Unable to process backend websocket connection.',
          error,
          details: { logId: numericLogId, radioId: numericRadioId, phase },
        });
      },
    };
  }, [
    onRefreshContactsRef,
    onSocketOpenRef,
    onSocketMessageRef,
    onRemoteContactRef,
    onRemoteContactDeletedRef,
    numericLogId,
    numericRadioId,
  ]);

  // Session changes retire the old connection during commit, before callbacks
  // from the replacement session can be delivered by the browser.
  useLayoutEffect(() => {
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
      logSocketDebug(isSocketDebugPanelEnabled, {
        event,
        sessionId,
        loggerId,
        logId: numericLogId,
        radioId: numericRadioId,
        ...details,
      });
    }

    const controller = createReconnectingSocket({
      url: websocketUrl('/ws', {
        session_id: sessionId,
        logger_id: loggerId,
        log_id: numericLogId,
        radio_id: numericRadioId,
      }),
      immediateTimeoutRecovery: true,
      foregroundConnectingGraceMs: 3000,
      createPing: (requestId) => ({ type: 'ping', request_id: requestId }),
      isPong: (message) => message?.type === 'pong',
      getCallbacks: () => ({ ...callbacksRef.current, onDebug: debugSocket }),
    });
    controllerRef.current = controller;
    controller.start();
    return () => {
      controller.dispose();
      if (controllerRef.current === controller) controllerRef.current = null;
    };
  }, [
    isSocketDebugPanelEnabled,
    sessionId,
    loggerId,
    numericLogId,
    numericRadioId,
  ]);

  useEffect(() => {
    setScoreSummary(EMPTY_SCORE_SUMMARY);
  }, [numericLogId]);

  return {
    backendSocketStatus,
    scoreSummary,
    isSocketDebugPanelEnabled,
    socketDebugEntries,
    sendBackendMessage,
  };
}
