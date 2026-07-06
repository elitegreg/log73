import { useEffect, useRef, useState } from 'react';
import {
  CW_REPEAT_DELAY_MS,
  DEFAULT_MESSAGE_LABELS,
  createMessageRequestId,
  cwActiveTimeoutMs,
  messageActionForRadioMode,
  messageButtonIsSendable,
  modeIsPhone,
} from '../mainWindowHelpers';
import {
  activeMessageKeysFromRequests,
  addActiveMessageRequest,
  removeActiveMessageRequest,
} from './messageSendingState';

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
  onSendMessage,
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
    const timeoutMs = cwActiveTimeoutMs(radio?.cw_keyer_type);
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
    const sendableKeys = [];
    const labels = modeIsPhone(radioMode)
      ? (messageLabels?.voice ?? null)
      : (messageLabels?.cw ?? messageLabels);

    for (const key of keys) {
      const action = messageActionForRadioMode(
        radio?.cw_messages,
        radio?.voice_messages,
        mode,
        key,
        radioMode,
      );
      if (action && performMessageAction(action)) {
        continue;
      }

      const button = (labels?.[mode] ?? DEFAULT_MESSAGE_LABELS[mode]).find(
        (label) => label.key === key,
      );
      if (!messageButtonIsSendable(button)) continue;
      if (mode === 'run' && key === 'F1') {
        storeCurrentCqFrequency();
      }
      sendableKeys.push(key);
    }

    if (sendableKeys.length === 0) return null;

    const requestId = createMessageRequestId();
    markMessageKeyActive(requestId, sendableKeys);
    onSendMessage?.({
      request_id: requestId,
      mode,
      keys: sendableKeys,
      fields: values,
    });
    return requestId;
  }

  function sendSingleMessageKey(
    key,
    mode = messageModeKey,
    values = currentMessageFields(),
  ) {
    return sendMessageKeys([key], mode, values);
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

  function sendEsmKeys(keys, values = currentMessageFields()) {
    const shouldRepeatF1 =
      messageModeKey === 'run' &&
      keys.length === 1 &&
      keys[0] === 'F1' &&
      repeatRunF1;

    stopRepeat();
    const requestId = sendMessageKeys(keys, messageModeKey, values);
    if (!requestId) return;
    if (keys.includes('F2')) {
      markEsmExchangeSentForCurrentCallsign();
    }
    if (shouldRepeatF1) {
      repeatActiveRef.current = true;
      repeatRequestIdRef.current = requestId;
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
