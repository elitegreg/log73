import { useEffect, useRef, useState } from 'react';
import {
  CW_REPEAT_DELAY_MS,
  DEFAULT_MESSAGE_LABELS,
  createMessageRequestId,
  messageActionForRadioMode,
  messageActiveTimeoutMs,
  messageButtonIsSendable,
  modeIsDigital,
  modeIsPhone,
} from '../mainWindowHelpers';
import {
  activeMessageKeysFromRequests,
  addActiveMessageRequest,
  removeActiveMessageRequest,
} from './messageSendingState';
import {
  completionTrackedTextRequest,
  messageSendBatches,
  renderMessageForConfig,
} from '../../domain/messages';

export function useMessageSending({
  radio,
  radioMode,
  messageLabels,
  messageModeKey,
  messageSentEvent,
  currentMessageFields,
  currentCallsign,
  storeCurrentCqFrequency,
  markEsmExchangeSentForCurrentCallsign,
  clearEntryFields,
  messageSendingEnabled = true,
  onSendText,
  onStopKeying,
}) {
  const [repeatRunF1, setRepeatRunF1] = useState(false);
  const [activeMessageKeys, setActiveMessageKeys] = useState(() => new Set());
  const repeatActiveRef = useRef(false);
  const repeatRequestIdRef = useRef(null);
  const repeatTimeoutRef = useRef(null);
  const callSignValueRef = useRef('');
  const repeatSendRunF1Ref = useRef(() => {});
  const activeMessageRequestsRef = useRef(new Map());
  const activeMessageTimeoutsRef = useRef(new Map());

  function stopRepeat() {
    repeatActiveRef.current = false;
    repeatRequestIdRef.current = null;
    if (repeatTimeoutRef.current !== null) {
      window.clearTimeout(repeatTimeoutRef.current);
      repeatTimeoutRef.current = null;
    }
  }

  function clearMessageRequest(requestId) {
    const keys = activeMessageRequestsRef.current.get(requestId);
    if (!keys) return;
    activeMessageRequestsRef.current = removeActiveMessageRequest(
      activeMessageRequestsRef.current,
      requestId,
    );
    const timeoutId = activeMessageTimeoutsRef.current.get(requestId);
    if (timeoutId !== undefined) {
      window.clearTimeout(timeoutId);
      activeMessageTimeoutsRef.current.delete(requestId);
    }
    setActiveMessageKeys(
      activeMessageKeysFromRequests(activeMessageRequestsRef.current),
    );
  }

  function markMessageKeyActive(requestId, keys) {
    activeMessageRequestsRef.current = addActiveMessageRequest(
      activeMessageRequestsRef.current,
      requestId,
      keys,
    );
    setActiveMessageKeys(
      activeMessageKeysFromRequests(activeMessageRequestsRef.current),
    );
    const timeoutMs = messageActiveTimeoutMs(radioMode, radio?.cw_keyer_type);
    const timeoutId = window.setTimeout(
      () => clearMessageRequest(requestId),
      timeoutMs,
    );
    activeMessageTimeoutsRef.current.set(requestId, timeoutId);
  }

  function clearAllMessageRequests() {
    for (const timeoutId of activeMessageTimeoutsRef.current.values()) {
      window.clearTimeout(timeoutId);
    }
    activeMessageTimeoutsRef.current.clear();
    activeMessageRequestsRef.current.clear();
    setActiveMessageKeys(new Set());
  }

  function performMessageAction(action) {
    switch (
      String(action ?? '')
        .trim()
        .toLowerCase()
    ) {
      case 'clear':
        clearEntryFields();
        return true;
      default:
        return false;
    }
  }

  function sendMessageKeys(
    keys,
    mode = messageModeKey,
    values = currentMessageFields(),
  ) {
    const sendableMessages = [];
    const labels = modeIsPhone(radioMode)
      ? (messageLabels?.voice ?? null)
      : modeIsDigital(radioMode)
        ? (messageLabels?.digital ?? messageLabels?.cw ?? messageLabels)
        : (messageLabels?.cw ?? messageLabels);
    const config = modeIsPhone(radioMode)
      ? radio?.voice_messages
      : modeIsDigital(radioMode)
        ? (radio?.digital_messages ?? radio?.cw_messages)
        : radio?.cw_messages;

    for (const key of keys) {
      const action = messageActionForRadioMode(
        radio?.cw_messages,
        radio?.voice_messages,
        mode,
        key,
        radioMode,
        radio?.digital_messages,
      );
      if (action && performMessageAction(action)) {
        continue;
      }

      const button = (labels?.[mode] ?? DEFAULT_MESSAGE_LABELS[mode]).find(
        (label) => label.key === key,
      );
      if (!messageButtonIsSendable(button)) continue;
      const text = renderMessageForConfig(config, mode, key, values, {
        replaceNewline: modeIsDigital(radioMode),
      });
      if (!text) continue;
      if (mode === 'run' && key === 'F1') {
        storeCurrentCqFrequency();
      }
      sendableMessages.push({ key, text });
    }

    if (sendableMessages.length === 0) return [];
    if (!messageSendingEnabled) return [];

    const batches = messageSendBatches(
      sendableMessages,
      modeIsPhone(radioMode),
    );
    return batches.map(({ keys: batchKeys, text }) => {
      const requestId = createMessageRequestId();
      markMessageKeyActive(requestId, batchKeys);
      onSendText?.(completionTrackedTextRequest(requestId, text));
      return requestId;
    });
  }

  function sendSingleMessageKey(
    key,
    mode = messageModeKey,
    values = currentMessageFields(),
  ) {
    return sendMessageKeys([key], mode, values)[0] ?? null;
  }

  repeatSendRunF1Ref.current = () => {
    repeatRequestIdRef.current = sendSingleMessageKey('F1', 'run');
  };
  callSignValueRef.current = currentCallsign();

  function sendMessageKey(key) {
    const shouldRepeat =
      messageModeKey === 'run' && key === 'F1' && repeatRunF1;
    stopRepeat();
    const requestId = sendSingleMessageKey(key);
    if (!requestId) return;

    if (key === 'F2') {
      markEsmExchangeSentForCurrentCallsign();
    }

    if (shouldRepeat) {
      repeatActiveRef.current = true;
      repeatRequestIdRef.current = requestId;
    }
  }

  function sendEsmKeys(keys, exchangeValues) {
    const shouldRepeatF1 =
      messageModeKey === 'run' &&
      keys.length === 1 &&
      keys[0] === 'F1' &&
      repeatRunF1;
    const values =
      exchangeValues === undefined
        ? currentMessageFields()
        : currentMessageFields(exchangeValues);

    stopRepeat();
    const requestIds = sendMessageKeys(keys, messageModeKey, values);
    if (requestIds.length === 0) return;
    if (keys.includes('F2')) {
      markEsmExchangeSentForCurrentCallsign();
    }
    if (shouldRepeatF1) {
      repeatActiveRef.current = true;
      repeatRequestIdRef.current = requestIds[0];
    }
  }

  function stopMessageSending() {
    stopRepeat();
    clearAllMessageRequests();
    onStopKeying?.();
  }

  useEffect(
    () => () => {
      stopRepeat();
      clearAllMessageRequests();
    },
    [],
  );

  useEffect(() => {
    if (messageSentEvent?.requestId)
      clearMessageRequest(messageSentEvent.requestId);
    if (
      !repeatActiveRef.current ||
      !messageSentEvent?.requestId ||
      messageSentEvent.requestId !== repeatRequestIdRef.current
    )
      return;
    repeatTimeoutRef.current = window.setTimeout(() => {
      repeatTimeoutRef.current = null;
      if (!repeatActiveRef.current || callSignValueRef.current.trim() !== '') {
        stopRepeat();
        return;
      }
      repeatSendRunF1Ref.current();
    }, CW_REPEAT_DELAY_MS);
  }, [messageSentEvent]);

  return {
    repeatRunF1,
    setRepeatRunF1,
    activeMessageKeys,
    sendMessageKey,
    sendEsmKeys,
    stopMessageSending,
    stopRepeat,
    clearAllMessageRequests,
  };
}
