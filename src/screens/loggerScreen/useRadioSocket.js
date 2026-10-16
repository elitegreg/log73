import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { reportClientErrorLater } from '../../lib/errorReporting.js';
import { DEFAULT_RADIO_STATE } from '../loggerScreenHelpers.js';
import {
  radioSocketUrl,
  wsjtxReceiptMessage,
  wsjtxTargetUpdate,
} from './radioSocketController.js';

import { createReconnectingSocket } from './reconnectingSocketController.js';

export function useRadioSocket({
  numericRadioId,
  numericLogId,
  loggerId,
  radioWebsocketUrl,
  isClientRadio,
  notifyOperationalError,
  onWsjtXLoggedAdif,
}) {
  const [radioState, setRadioState] = useState(DEFAULT_RADIO_STATE);
  const [radioSocketStatus, setRadioSocketStatus] = useState('disconnected');
  const [catStatus, setCatStatus] = useState('offline');
  const [messageSentEvent, setMessageSentEvent] = useState(null);
  const [wsjtxTarget, setWsjtXTargetState] = useState(false);
  const [digitalIoTarget, setDigitalIoTargetState] = useState(false);
  const [digitalIoText, setDigitalIoText] = useState('');
  const controllerRef = useRef(null);
  const wsjtxTargetIntentRef = useRef(false);
  const digitalIoTargetIntentRef = useRef(false);
  const callbacksRef = useRef({});
  useLayoutEffect(() => {
    callbacksRef.current = {
      notifyOperationalError,
      onWsjtXLoggedAdif,
      isClientRadio,
    };
  }, [notifyOperationalError, onWsjtXLoggedAdif, isClientRadio]);

  const sendRadioMessage = useCallback((message) => {
    controllerRef.current?.send(message);
  }, []);

  const setWsjtXTarget = useCallback(
    (enabled) => {
      const nextEnabled = Boolean(enabled);
      wsjtxTargetIntentRef.current = nextEnabled;
      setWsjtXTargetState(nextEnabled);
      sendRadioMessage({ type: 'set_wsjtx_target', enabled: nextEnabled });
    },
    [sendRadioMessage],
  );

  const setDigitalIoTarget = useCallback(
    (enabled) => {
      const nextEnabled = Boolean(enabled);
      digitalIoTargetIntentRef.current = nextEnabled;
      setDigitalIoTargetState(nextEnabled);
      sendRadioMessage({ type: 'set_digital_io_target', enabled: nextEnabled });
    },
    [sendRadioMessage],
  );

  const clearDigitalIo = useCallback(() => {
    setDigitalIoText('');
    sendRadioMessage({ type: 'digital_io_clear' });
  }, [sendRadioMessage]);

  useLayoutEffect(() => {
    setRadioState(DEFAULT_RADIO_STATE);
    setMessageSentEvent(null);
    setWsjtXTargetState(false);
    setDigitalIoTargetState(false);
    setDigitalIoText('');
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

    let url;
    try {
      url = radioSocketUrl(radioWebsocketUrl, {
        loggerId,
        logId: numericLogId,
      });
    } catch (error) {
      setRadioSocketStatus('disconnected');
      setCatStatus('offline');
      callbacksRef.current.notifyOperationalError(
        'radioSocketUrl',
        'Unable to connect to radio I/O.',
        error,
        { radioId: numericRadioId, radioWebsocketUrl },
      );
      return undefined;
    }

    const controller = createReconnectingSocket({
      url,
      createPing: (requestId) => ({ type: 'ping', request_id: requestId }),
      isPong: (message) => message?.type === 'pong',
      getCallbacks: () => ({
        onStatusChange(status) {
          setRadioSocketStatus(status);
          if (status !== 'connected') setCatStatus('offline');
        },
        onOpen({ send }) {
          if (wsjtxTargetIntentRef.current)
            send({ type: 'set_wsjtx_target', enabled: true });
          if (digitalIoTargetIntentRef.current)
            send({ type: 'set_digital_io_target', enabled: true });
        },
        onMessage(message, { send }) {
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
              send({ type: 'set_wsjtx_target', enabled: true });
            }
          } else if (message.type === 'wsjtx_logged_adif') {
            if (Number(message.log_id) !== numericLogId) {
              callbacksRef.current.notifyOperationalError(
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
              const accepted =
                callbacksRef.current.onWsjtXLoggedAdif?.(message);
              const receipt = wsjtxReceiptMessage(
                message,
                numericLogId,
                accepted,
              );
              if (receipt) send(receipt);
            }
          } else if (message.type === 'wsjtx_error') {
            callbacksRef.current.notifyOperationalError(
              'wsjtx',
              'WSJT-X integration error.',
              message.message,
              { logId: numericLogId, radioId: numericRadioId },
            );
          } else if (message.type === 'digital_io_target') {
            const isTarget =
              String(message.logger_id ?? '') === loggerId &&
              Number(message.log_id) === numericLogId;
            setDigitalIoTargetState(isTarget);
            if (message.logger_id) {
              digitalIoTargetIntentRef.current = isTarget;
            } else if (digitalIoTargetIntentRef.current) {
              send({ type: 'set_digital_io_target', enabled: true });
            }
          } else if (message.type === 'digital_io_received') {
            if (Number(message.log_id) === numericLogId) {
              setDigitalIoText((current) =>
                `${current}${String(message.text ?? '')}`.slice(-65536),
              );
            }
          } else if (message.type === 'digital_io_error') {
            callbacksRef.current.notifyOperationalError(
              'digitalIo',
              'Digital I/O integration error.',
              message.message,
              { logId: numericLogId, radioId: numericRadioId },
            );
          }
        },
        onMessageError(error, raw) {
          if (messageIsWsjtXLoggedAdif(raw)) {
            callbacksRef.current.notifyOperationalError(
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
        },
        onClose() {
          setWsjtXTargetState(false);
          setDigitalIoTargetState(false);
        },
        onError(error, { phase }) {
          if (phase === 'socket' && callbacksRef.current.isClientRadio) {
            callbacksRef.current.notifyOperationalError(
              'clientRadioSocketConnect',
              'log73-radio-client is not running on this computer or this is another operator’s client-side radio.',
              null,
              { logId: numericLogId, radioId: numericRadioId },
            );
          } else if (phase !== 'socket') {
            callbacksRef.current.notifyOperationalError(
              'radioSocketConnect',
              'Unable to connect to radio I/O.',
              error,
              { radioId: numericRadioId, url },
            );
          }
        },
      }),
    });
    controllerRef.current = controller;
    controller.start();
    return () => {
      controller.dispose();
      if (controllerRef.current === controller) controllerRef.current = null;
      setCatStatus('offline');
    };
  }, [loggerId, numericLogId, numericRadioId, radioWebsocketUrl]);

  return {
    radioState,
    radioSocketStatus,
    catStatus,
    messageSentEvent,
    sendRadioMessage,
    wsjtxTarget,
    setWsjtXTarget,
    digitalIoTarget,
    digitalIoText,
    setDigitalIoTarget,
    clearDigitalIo,
  };
}

function messageIsWsjtXLoggedAdif(raw) {
  try {
    return JSON.parse(raw)?.type === 'wsjtx_logged_adif';
  } catch {
    return false;
  }
}
