import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  callsignCompletionMatches,
  exchangeCompletionMatches,
} from '../domain/completions';
import {
  buildSentExchange,
  fieldDefault,
  sanitizeCallsign,
  sanitizeExchangeValue,
} from '../domain/contactFields';
import {
  lastCqFrequencyForBand,
  nextBandMapSpotAbove,
  nextBandMapSpotBelow,
} from '../domain/bandMap';
import { dxccLabel, lookupDxcc } from '../domain/dxcc';
import { dupeAlertText } from '../domain/dupes';
import { validateCallsign, validateExchangeField } from '../domain/validation';
import { dxcc, supercheckpartial } from '../lib/api';
import {
  CW_WPM_STORAGE_KEY,
  ESM_ENABLED_STORAGE_KEY,
  DEFAULT_MESSAGE_LABELS,
  DEFAULT_CW_WPM,
  CW_WPM_MIN,
  CW_WPM_MAX,
  DEFAULT_RADIO_FREQUENCY_HZ,
  SUPERCHECKPARTIAL_MIN_QUERY_LENGTH,
  CALLSIGN_LOOKUP_DEBOUNCE_MS,
  CW_REPEAT_DELAY_MS,
  FUNCTION_KEY_PATTERN,
  HZ_PER_KHZ,
  EPOCH_MS_PER_SECOND,
  nextCwWpm,
  cwActiveTimeoutMs,
  typedModeFromCallsignInput,
  exchangeDefaults,
  previousContactExchangeAutofill,
  formatFrequency,
  isFrequencyInput,
  adifModeForLoggerMode,
  isSelectableMode,
  modeIsCw,
  esmEnterAction,
  bandForFrequency,
  bandByMeters,
  createContactId,
  createMessageRequestId,
  isEmptyMessageButton,
  cwActionForMessage,
  callsignHasQuery,
  shouldBlockEsmCallEnter,
  callsignClearThresholdHz,
  normalizedContactFrequencyHz,
  shouldAdvanceFromCallsignAutofill,
  tuningIncrementHzForMode,
  steppedFrequencyHz,
} from './mainWindowHelpers';
import RadioControls from './components/RadioControls';
import EntryFields from './components/EntryFields';
import FunctionKeys from './components/FunctionKeys';
import CommandButtons from './components/CommandButtons';
import StatusBar from './components/StatusBar';

function MainWindow({
  settings,
  log,
  radio,
  isContextLoading,
  contactsLoadState,
  contacts = [],
  lastContact = null,
  stationCallsign,
  operatorCallsign,
  radioState,
  backendSocketStatus,
  catStatus,
  messageLabels,
  messageSentEvent,
  sessionId,
  logId,
  bandMapEnabled,
  bandMapSpotStore,
  bandMapSelection,
  onSetBandMapEnabled,
  onActivateBandMapSpot,
  onStoreCqFrequency,
  onMarkFrequency,
  onStoreBandMapSpot,
  onSetRadioFrequency,
  onSetRadioMode,
  onClearRit,
  onIncrementRit,
  onDecrementRit,
  onSendMessage,
  onSendCwText,
  onSendDxClusterSpot,
  onStopCw,
  onSetCwWpm,
  onLogContact,
  onDebouncedCallsignChange,
  onRescore,
  isRescoreLoading,
  scoreSummary,
  serialAllocation,
  onSerialContactLogged,
  onExit,
}) {
  const [callSign, setCallSign] = useState('');
  const [debouncedCallSign, setDebouncedCallSign] = useState('');
  const [exchangeValues, setExchangeValues] = useState({});
  const [operatingMode, setOperatingMode] = useState('S&P');
  const [repeatRunF1, setRepeatRunF1] = useState(false);
  const [esmEnabled, setEsmEnabled] = useState(() => {
    return localStorage.getItem(ESM_ENABLED_STORAGE_KEY) === '1';
  });
  const [esmRunCallsignAttempt, setEsmRunCallsignAttempt] = useState('');
  const [esmExchangeSentCallsign, setEsmExchangeSentCallsign] = useState('');
  const [activeMessageKeys, setActiveMessageKeys] = useState(() => new Set());
  const [activeCompletionField, setActiveCompletionField] = useState(null);
  const [supercheckpartialCallsigns, setSupercheckpartialCallsigns] = useState(
    [],
  );
  const [supercheckpartialMatches, setSupercheckpartialMatches] = useState([]);
  const [dxccData, setDxccData] = useState(null);
  const [cwWpm, setCwWpm] = useState(() => {
    const storedWpm = Number.parseInt(
      localStorage.getItem(CW_WPM_STORAGE_KEY) ?? '',
      10,
    );
    return Number.isFinite(storedWpm) ? storedWpm : DEFAULT_CW_WPM;
  });
  const [isCwTextDialogOpen, setIsCwTextDialogOpen] = useState(false);
  const [cwTextCommittedWords, setCwTextCommittedWords] = useState([]);
  const [cwTextCurrentWord, setCwTextCurrentWord] = useState('');
  const callSignRef = useRef(null);
  const setCwWpmRef = useRef(onSetCwWpm);
  const repeatActiveRef = useRef(false);
  const repeatRequestIdRef = useRef(null);
  const repeatTimeoutRef = useRef(null);
  const callSignValueRef = useRef('');
  const repeatSendRunF1Ref = useRef(() => {});
  const callsignSelectionRef = useRef(null);
  const activeMessageRequestsRef = useRef(new Map());
  const activeMessageTimeoutsRef = useRef(new Map());
  const exchangeInputRefs = useRef({});
  const cwTextInputRef = useRef(null);
  const callSignEditedAtRef = useRef(new Date());
  const callsignFrequencyBaselineRef = useRef(null);
  const pendingBandMapTuneFrequencyRef = useRef(null);
  const pendingPreviousContactAutofillRef = useRef('');
  const bandMapSelectionSequenceRef = useRef(null);
  const radioMode = radioState?.mode ?? 'CW';
  const radioFrequencyHz =
    radioState?.frequency_hz ?? DEFAULT_RADIO_FREQUENCY_HZ;
  const allowedBands = settings?.allowed_bands ?? [];
  const currentBand = bandForFrequency(radioFrequencyHz);
  const currentBandValue = currentBand ? String(currentBand.meters) : 'unknown';
  const currentBandAllowed = currentBand
    ? allowedBands.includes(currentBand.meters)
    : false;
  const bandOptions = allowedBands.map(bandByMeters).filter(Boolean);
  const loadingStatus = isContextLoading
    ? 'Loading logger context...'
    : contactsLoadState === 'initial-loading'
      ? 'Loading contacts...'
      : contactsLoadState === 'refreshing'
        ? 'Refreshing contacts...'
        : contactsLoadState === 'retrying'
          ? 'Retrying contact load...'
          : '';

  if (
    currentBand &&
    !bandOptions.some((band) => band.meters === currentBand.meters)
  ) {
    bandOptions.push(currentBand);
  }

  useEffect(() => {
    setExchangeValues(
      exchangeDefaults(settings, radioMode, log?.contest_params ?? {}),
    );
  }, [settings, radioMode, log]);

  useEffect(() => {
    if (!serialAllocation?.required || !serialAllocation.fieldAdif) return;
    const serialField = (settings?.exchange ?? []).find(
      (field) => field.is_sent && field.adif === serialAllocation.fieldAdif,
    );
    if (!serialField) return;
    setExchangeValues((currentValues) => ({
      ...currentValues,
      [serialField.name]:
        serialAllocation.current === null ||
        serialAllocation.current === undefined
          ? ''
          : String(serialAllocation.current),
    }));
  }, [settings, serialAllocation]);

  useEffect(() => {
    setCwWpmRef.current = onSetCwWpm;
  });

  useEffect(() => {
    localStorage.setItem(CW_WPM_STORAGE_KEY, String(cwWpm));
    setCwWpmRef.current?.(cwWpm);
  }, [cwWpm]);

  useEffect(() => {
    localStorage.setItem(ESM_ENABLED_STORAGE_KEY, esmEnabled ? '1' : '0');
  }, [esmEnabled]);

  useEffect(() => {
    if (backendSocketStatus === 'connected') {
      setCwWpmRef.current?.(cwWpm);
    }
  }, [backendSocketStatus, cwWpm]);

  useEffect(() => {
    const selection = callsignSelectionRef.current;
    const input = callSignRef.current;
    if (!selection || !input || document.activeElement !== input) return;

    const start = Math.min(selection.start, input.value.length);
    const end = Math.min(selection.end, input.value.length);
    input.setSelectionRange(start, end);
    callsignSelectionRef.current = null;
  }, [callSign]);

  useEffect(() => {
    if (isCwTextDialogOpen) {
      cwTextInputRef.current?.focus();
    }
  }, [isCwTextDialogOpen]);

  useEffect(() => {
    if (callSign.trim() === '') {
      setDebouncedCallSign('');
      return;
    }

    const timeoutId = window.setTimeout(() => {
      setDebouncedCallSign(callSign);
    }, CALLSIGN_LOOKUP_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [callSign]);

  useEffect(() => {
    onDebouncedCallsignChange?.(debouncedCallSign.trim().toUpperCase());
  }, [debouncedCallSign, onDebouncedCallsignChange]);

  useEffect(() => {
    const pendingCallsign = pendingPreviousContactAutofillRef.current;
    if (!pendingCallsign || !settings?.exchange) return;

    const normalizedCallsign = callSign.trim().toUpperCase();
    if (pendingCallsign !== normalizedCallsign) {
      pendingPreviousContactAutofillRef.current = '';
      return;
    }

    const autofillResult = previousContactExchangeAutofill({
      settings,
      contacts,
      callsign: normalizedCallsign,
      exchangeValues,
      radioMode,
      contestParams: log?.contest_params ?? {},
    });

    if (!autofillResult.matchedContact) return;

    pendingPreviousContactAutofillRef.current = '';
    if (autofillResult.changed) {
      setExchangeValues(autofillResult.values);
    }
  }, [callSign, contacts, exchangeValues, log, radioMode, settings]);

  useEffect(() => {
    let cancelled = false;
    supercheckpartial()
      .then((result) => {
        if (!cancelled) {
          setSupercheckpartialCallsigns(
            Array.isArray(result.callsigns) ? result.callsigns : [],
          );
        }
      })
      .catch(() => {
        if (!cancelled) {
          setSupercheckpartialCallsigns([]);
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    dxcc()
      .then((result) => {
        if (!cancelled) {
          setDxccData(result?.ok ? (result.dxcc ?? null) : null);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setDxccData(null);
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const combinedSupercheckpartialCallsigns = useMemo(() => {
    const callsigns = new Set(
      supercheckpartialCallsigns.map((callsign) =>
        String(callsign ?? '')
          .trim()
          .toUpperCase(),
      ),
    );
    for (const spot of bandMapSpotStore?.sortedSpots ?? []) {
      const callsign = String(spot?.call_dx ?? '')
        .trim()
        .toUpperCase();
      if (callsign) callsigns.add(callsign);
    }
    return [...callsigns].filter(Boolean);
  }, [supercheckpartialCallsigns, bandMapSpotStore]);

  useEffect(() => {
    if (activeCompletionField !== 'CALL') {
      setSupercheckpartialMatches([]);
      return;
    }

    const query = debouncedCallSign.trim().toUpperCase();
    if (query.length < SUPERCHECKPARTIAL_MIN_QUERY_LENGTH) {
      setSupercheckpartialMatches([]);
      return;
    }

    setSupercheckpartialMatches(
      callsignCompletionMatches(combinedSupercheckpartialCallsigns, query),
    );
  }, [
    activeCompletionField,
    debouncedCallSign,
    combinedSupercheckpartialCallsigns,
  ]);

  const messageModeKey = operatingMode === 'Run' ? 'run' : 's&p';
  const activeMessageLabels =
    messageLabels?.[messageModeKey] ?? DEFAULT_MESSAGE_LABELS[messageModeKey];
  const activeExchangeCompletionField = (settings?.exchange ?? []).find(
    (field) => field.name === activeCompletionField && field.fixed !== true,
  );
  const completionMatches =
    activeCompletionField === 'CALL'
      ? supercheckpartialMatches
      : exchangeCompletionMatches(
          activeExchangeCompletionField,
          exchangeValues[activeExchangeCompletionField?.name],
        );
  const currentDxccInfo = lookupDxcc(dxccData, debouncedCallSign);
  const currentDxccLabel = dxccLabel(currentDxccInfo);
  const currentDupeAlertText =
    callSign.trim() !== '' &&
    debouncedCallSign.trim().toUpperCase() === callSign.trim().toUpperCase()
      ? dupeAlertText(settings, currentContactFields(), contacts)
      : '';

  function currentMessageFields(values = exchangeValues) {
    const fields = {
      STATION_CALLSIGN: stationCallsign,
      CALL: callSign.trim().toUpperCase(),
    };

    for (const field of settings?.exchange ?? []) {
      fields[field.adif] = String(
        values?.[field.name] ??
          fieldDefault(field, radioMode, log?.contest_params ?? {}),
      )
        .trim()
        .toUpperCase();
    }

    fields.EXCH = buildSentExchange(
      settings,
      values,
      radioMode,
      log?.contest_params ?? {},
    );

    return fields;
  }

  function currentContactFields(values = exchangeValues) {
    const contact = {
      CALL: callSign.trim().toUpperCase(),
      BAND: currentBand?.name ?? '',
      FREQ: radioFrequencyHz,
      MODE: adifModeForLoggerMode(radioMode),
    };

    for (const field of settings?.exchange ?? []) {
      contact[field.adif] = String(exchangeValue(field, values))
        .trim()
        .toUpperCase();
    }

    return contact;
  }

  function currentCallsign() {
    return callSign.trim().toUpperCase();
  }

  function requestPreviousContactAutofill(values = exchangeValues) {
    const normalizedCallsign = currentCallsign();
    if (!normalizedCallsign) {
      return {
        matchedContact: null,
        changed: false,
        copiedFields: [],
        values,
      };
    }

    pendingPreviousContactAutofillRef.current = normalizedCallsign;
    setDebouncedCallSign(normalizedCallsign);

    if (!settings?.exchange) {
      return {
        matchedContact: null,
        changed: false,
        copiedFields: [],
        values,
      };
    }

    const autofillResult = previousContactExchangeAutofill({
      settings,
      contacts,
      callsign: normalizedCallsign,
      exchangeValues: values,
      radioMode,
      contestParams: log?.contest_params ?? {},
    });

    if (autofillResult.matchedContact) {
      pendingPreviousContactAutofillRef.current = '';
      if (autofillResult.changed) {
        setExchangeValues(autofillResult.values);
      }
    }

    return autofillResult;
  }

  function storeCurrentCqFrequency() {
    if (!currentBand) return;
    onStoreCqFrequency?.(radioFrequencyHz, currentBand.meters);
  }

  function jumpToLastCqFrequency() {
    const frequencyHz = lastCqFrequencyForBand(
      bandMapSpotStore,
      currentBand?.meters,
    );
    if (frequencyHz) onSetRadioFrequency?.(frequencyHz);
  }

  function markCurrentFrequency() {
    onMarkFrequency?.(radioFrequencyHz);
  }

  function storeCurrentBandMapSpot() {
    const callsign = currentCallsign();
    if (!callsign) return;
    onStoreBandMapSpot?.({
      frequency_hz: radioFrequencyHz,
      call: callsign,
      comment: '',
    });
  }

  function activateBandMapSpot(spot) {
    if (!spot) return;
    onActivateBandMapSpot?.(spot);
  }

  function clearRitIfEnabled() {
    if (!radio?.rit_clear_on_log) return;
    onClearRit?.();
  }

  function tuningIncrementHz() {
    return tuningIncrementHzForMode(radio, radioMode);
  }

  function tuneByIncrement(direction) {
    const incrementHz = tuningIncrementHz();
    if (incrementHz <= 0) return;

    const isRunMode = operatingMode === 'Run';
    if (isRunMode && onIncrementRit && onDecrementRit) {
      if (direction > 0) onIncrementRit(incrementHz);
      else onDecrementRit(incrementHz);
      return;
    }

    const deltaHz = direction > 0 ? incrementHz : -incrementHz;
    onSetRadioFrequency?.(steppedFrequencyHz(radioFrequencyHz, deltaHz));
  }

  function shiftBand(direction) {
    if (!currentBand || bandOptions.length === 0) return;

    const sortedBands = [
      ...new Map(bandOptions.map((band) => [band.meters, band])).values(),
    ].sort((left, right) => left.lowerHz - right.lowerHz);
    const currentIndex = sortedBands.findIndex(
      (band) => band.meters === currentBand.meters,
    );
    if (currentIndex === -1) return;

    const nextIndex = currentIndex + direction;
    if (nextIndex < 0 || nextIndex >= sortedBands.length) return;

    const nextBand = sortedBands[nextIndex];
    onSetRadioFrequency?.(nextBand.lowerHz);
    if (isSelectableMode(radioMode)) {
      onSetRadioMode?.(radioMode);
    }
  }

  function clearEsmState() {
    setEsmRunCallsignAttempt('');
    setEsmExchangeSentCallsign('');
  }

  useEffect(() => {
    if (!bandMapSelection?.spot) return;
    if (bandMapSelectionSequenceRef.current === bandMapSelection.sequence)
      return;
    bandMapSelectionSequenceRef.current = bandMapSelection.sequence;
    const callsign = sanitizeCallsign(bandMapSelection.spot?.call_dx ?? '');
    setCallSign(callsign);
    pendingPreviousContactAutofillRef.current = '';
    callsignFrequencyBaselineRef.current =
      Number(bandMapSelection.spot?.frequency_hz) || null;
    pendingBandMapTuneFrequencyRef.current =
      callsignFrequencyBaselineRef.current;
    setEsmRunCallsignAttempt('');
    setEsmExchangeSentCallsign('');
    callSignEditedAtRef.current = new Date();
    window.requestAnimationFrame(() => callSignRef.current?.focus());
  }, [bandMapSelection]);

  useEffect(() => {
    const baseline = callsignFrequencyBaselineRef.current;
    if (!callSign.trim() || !baseline) return;
    const thresholdHz = callsignClearThresholdHz(radioMode);
    const pendingBandMapTuneFrequency = pendingBandMapTuneFrequencyRef.current;
    if (pendingBandMapTuneFrequency) {
      if (
        Math.abs(radioFrequencyHz - pendingBandMapTuneFrequency) < thresholdHz
      ) {
        pendingBandMapTuneFrequencyRef.current = null;
      }
      return;
    }
    if (Math.abs(radioFrequencyHz - baseline) < thresholdHz) return;
    setCallSign('');
    pendingPreviousContactAutofillRef.current = '';
    callsignFrequencyBaselineRef.current = null;
    setEsmRunCallsignAttempt('');
    setEsmExchangeSentCallsign('');
    callSignEditedAtRef.current = new Date();
  }, [callSign, radioFrequencyHz, radioMode]);

  function markEsmExchangeSentForCurrentCallsign() {
    const normalizedCallsign = currentCallsign();
    if (!normalizedCallsign) return;
    setEsmExchangeSentCallsign(normalizedCallsign);
  }

  function stopRepeat() {
    repeatActiveRef.current = false;
    repeatRequestIdRef.current = null;
    if (repeatTimeoutRef.current !== null) {
      window.clearTimeout(repeatTimeoutRef.current);
      repeatTimeoutRef.current = null;
    }
  }

  function markMessageKeyActive(requestId, keys) {
    activeMessageRequestsRef.current.set(requestId, [...keys]);
    setActiveMessageKeys((current) => {
      const next = new Set(current);
      for (const key of keys) next.add(key);
      return next;
    });
    const timeoutMs = cwActiveTimeoutMs(radio?.cw_keyer_type);
    const timeoutId = window.setTimeout(
      () => clearMessageRequest(requestId),
      timeoutMs,
    );
    activeMessageTimeoutsRef.current.set(requestId, timeoutId);
  }

  function clearMessageRequest(requestId) {
    const keys = activeMessageRequestsRef.current.get(requestId);
    if (!keys) return;
    activeMessageRequestsRef.current.delete(requestId);
    const timeoutId = activeMessageTimeoutsRef.current.get(requestId);
    if (timeoutId !== undefined) {
      window.clearTimeout(timeoutId);
      activeMessageTimeoutsRef.current.delete(requestId);
    }
    setActiveMessageKeys((current) => {
      const remainingKeys = new Set();
      for (const activeKeys of activeMessageRequestsRef.current.values()) {
        for (const key of activeKeys) remainingKeys.add(key);
      }
      const next = new Set(current);
      for (const key of keys) {
        if (!remainingKeys.has(key)) next.delete(key);
      }
      return next;
    });
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
    values = exchangeValues,
  ) {
    const sendableKeys = [];

    for (const key of keys) {
      const action = cwActionForMessage(radio?.cw_messages, mode, key);
      if (action && performMessageAction(action)) {
        continue;
      }

      const button = (
        messageLabels?.[mode] ?? DEFAULT_MESSAGE_LABELS[mode]
      ).find((label) => label.key === key);
      if (isEmptyMessageButton(button)) continue;
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
      fields: currentMessageFields(values),
    });
    return requestId;
  }

  function sendSingleMessageKey(
    key,
    mode = messageModeKey,
    values = exchangeValues,
  ) {
    return sendMessageKeys([key], mode, values);
  }

  repeatSendRunF1Ref.current = () => {
    repeatRequestIdRef.current = sendSingleMessageKey('F1', 'run');
  };
  callSignValueRef.current = callSign;

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

    if (messageModeKey === 's&p' && key === 'F1') {
      setOperatingMode('Run');
    }
  }

  function stopMessageSending() {
    stopRepeat();
    clearAllMessageRequests();
    onStopCw?.();
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

  function openCwTextDialog() {
    if (!modeIsCw(radioMode)) return;
    setCwTextCommittedWords([]);
    setCwTextCurrentWord('');
    setIsCwTextDialogOpen(true);
  }

  function closeCwTextDialog() {
    setIsCwTextDialogOpen(false);
    setCwTextCommittedWords([]);
    setCwTextCurrentWord('');
    callSignRef.current?.focus();
  }

  function sendCwTextWord(sendTrailingSpace) {
    const word = cwTextCurrentWord.trim().toUpperCase();
    if (!word) return;

    onSendCwText?.({
      request_id: createMessageRequestId(),
      text: sendTrailingSpace ? `${word} ` : word,
      wait_for_completion: false,
    });
    setCwTextCommittedWords((current) => [...current, word]);
    setCwTextCurrentWord('');
  }

  function handleCwTextInputChange(event) {
    setCwTextCurrentWord(String(event.target.value ?? '').replace(/\s+/g, ''));
  }

  function handleCwTextInputKeyDown(event) {
    if (event.key === ' ') {
      event.preventDefault();
      sendCwTextWord(true);
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      sendCwTextWord(false);
      closeCwTextDialog();
      return;
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      closeCwTextDialog();
      return;
    }

    if (event.key === 'Backspace' && cwTextCurrentWord.length === 0) {
      event.preventDefault();
    }
  }

  function sendEsmKeys(keys, values = exchangeValues) {
    const shouldRepeatF1 =
      messageModeKey === 'run' && keys.length === 1 && keys[0] === 'F1' && repeatRunF1;

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

  useEffect(() => {
    if (!modeIsCw(radioMode) && isCwTextDialogOpen) {
      setIsCwTextDialogOpen(false);
      setCwTextCommittedWords([]);
      setCwTextCurrentWord('');
      callSignRef.current?.focus();
    }
  }, [radioMode, isCwTextDialogOpen]);

  useEffect(() => {
    function handleFunctionKey(event) {
      if (
        event.altKey &&
        !event.ctrlKey &&
        !event.metaKey &&
        event.key.toLowerCase() === 'm'
      ) {
        event.preventDefault();
        markCurrentFrequency();
        return;
      }
      if (
        event.altKey &&
        !event.ctrlKey &&
        !event.metaKey &&
        event.key.toLowerCase() === 'o'
      ) {
        event.preventDefault();
        storeCurrentBandMapSpot();
        return;
      }
      if (
        event.altKey &&
        !event.ctrlKey &&
        !event.metaKey &&
        event.key.toLowerCase() === 'q'
      ) {
        event.preventDefault();
        jumpToLastCqFrequency();
        return;
      }
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key.toLowerCase() === 'p'
      ) {
        event.preventDefault();
        handleSpotIt();
        return;
      }
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        (event.key === 'ArrowDown' || event.key === 'ArrowUp')
      ) {
        event.preventDefault();
        const spot =
          event.key === 'ArrowDown'
            ? nextBandMapSpotAbove(bandMapSpotStore, radioFrequencyHz)
            : nextBandMapSpotBelow(bandMapSpotStore, radioFrequencyHz);
        if (spot) activateBandMapSpot(spot);
        return;
      }
      if (event.target?.closest?.('.log-window')) return;
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key.toLowerCase() === 'k' &&
        modeIsCw(radioMode)
      ) {
        event.preventDefault();
        openCwTextDialog();
        return;
      }
      if (isCwTextDialogOpen) {
        if (event.key === 'Escape') {
          event.preventDefault();
          closeCwTextDialog();
        }
        return;
      }
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key === 'PageUp'
      ) {
        event.preventDefault();
        shiftBand(1);
        return;
      }
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key === 'PageDown'
      ) {
        event.preventDefault();
        shiftBand(-1);
        return;
      }
      if (
        !event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        (event.key === 'ArrowUp' || event.key === 'ArrowDown')
      ) {
        event.preventDefault();
        tuneByIncrement(event.key === 'ArrowUp' ? 1 : -1);
        return;
      }
      if (event.key === 'Escape') {
        event.preventDefault();
        stopMessageSending();
        return;
      }
      if (
        !event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key === 'PageUp'
      ) {
        event.preventDefault();
        setCwWpm((current) => nextCwWpm(current, 1));
        return;
      }
      if (
        !event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key === 'PageDown'
      ) {
        event.preventDefault();
        setCwWpm((current) => nextCwWpm(current, -1));
        return;
      }
      if (FUNCTION_KEY_PATTERN.test(event.key)) {
        event.preventDefault();
        sendMessageKey(event.key);
      }
    }

    window.addEventListener('keydown', handleFunctionKey);
    return () => window.removeEventListener('keydown', handleFunctionKey);
  });

  function updateExchangeField(field, value) {
    setExchangeValues((current) => ({
      ...current,
      [field.name]: sanitizeExchangeValue(field, value, radioMode),
    }));
  }

  function handleCallsignChange(event) {
    stopRepeat();
    const { selectionStart, selectionEnd } = event.target;
    callsignSelectionRef.current = {
      start: selectionStart ?? event.target.value.length,
      end: selectionEnd ?? event.target.value.length,
    };
    const sanitizedCallsign = sanitizeCallsign(event.target.value);
    const normalizedCallsign = sanitizedCallsign.trim().toUpperCase();
    if (normalizedCallsign !== esmRunCallsignAttempt) {
      setEsmRunCallsignAttempt('');
    }
    setCallSign(sanitizedCallsign);
    pendingPreviousContactAutofillRef.current = '';
    callsignFrequencyBaselineRef.current = normalizedCallsign
      ? radioFrequencyHz
      : null;
    pendingBandMapTuneFrequencyRef.current = null;
    callSignEditedAtRef.current = new Date();
  }

  function exchangeValue(field, values = exchangeValues) {
    return (
      values?.[field.name] ??
      fieldDefault(field, radioMode, log?.contest_params ?? {})
    );
  }

  function fieldEditable(field) {
    const typeKind = String(field?.type ?? '')
      .split(':')[0]
      .trim()
      .toUpperCase();
    return field?.fixed !== true && !(field?.is_sent && typeKind === 'SERIAL');
  }

  function exchangeValidation(field, values = exchangeValues) {
    return validateExchangeField(
      field,
      exchangeValue(field, values),
      radioMode,
    );
  }

  function firstInvalidExchangeField(values = exchangeValues) {
    return (settings?.exchange ?? []).find(
      (field) => !exchangeValidation(field, values).ok,
    );
  }

  function allRequiredFieldsFilled(values = exchangeValues) {
    return (
      Boolean(settings?.exchange) &&
      callSign.trim() !== '' &&
      settings.exchange.every(
        (field) => String(exchangeValue(field, values)).trim() !== '',
      )
    );
  }

  function callsignValidation() {
    return validateCallsign(callSign);
  }

  const serialBlockMessage =
    serialAllocation?.required && !serialAllocation.available
      ? serialAllocation.message ||
        'No serial number is currently available. Waiting for backend allocation.'
      : '';

  function canLogContact(force = false, values = exchangeValues) {
    return (
      !serialBlockMessage &&
      allRequiredFieldsFilled(values) &&
      (force || (callsignValidation().ok && !firstInvalidExchangeField(values)))
    );
  }

  function clearEntryFields({ clearRit = true } = {}) {
    if (clearRit) clearRitIfEnabled();
    setCallSign('');
    callsignFrequencyBaselineRef.current = null;
    pendingBandMapTuneFrequencyRef.current = null;
    pendingPreviousContactAutofillRef.current = '';
    setExchangeValues(
      exchangeDefaults(settings, radioMode, log?.contest_params ?? {}),
    );
    clearEsmState();
    callSignEditedAtRef.current = new Date();
    callSignRef.current?.focus();
  }

  function logContact(force = false, values = exchangeValues) {
    if (!canLogContact(force, values)) {
      if (!force && !callsignValidation().ok) {
        callSignRef.current?.focus();
        return false;
      }

      const invalidField = firstInvalidExchangeField(values);
      if (invalidField) {
        exchangeInputRefs.current[invalidField.name]?.focus();
      }
      return false;
    }

    const timeOn = callSignEditedAtRef.current;
    const normalizedCallSign = callSign.trim().toUpperCase();
    const contact = {
      QSO_DATE_TIME_ON: Math.floor(timeOn.getTime() / EPOCH_MS_PER_SECOND),
      STATION_CALLSIGN: stationCallsign,
      OPERATOR: operatorCallsign,
      CONTEST_ID: settings.contest,
      CALL: normalizedCallSign,
      BAND: currentBand?.name ?? '',
      FREQ: radioFrequencyHz,
      MODE: adifModeForLoggerMode(radioMode),
      _status: 'Pending',
      _session_id: sessionId,
      _log_id: logId,
      _client_id: createContactId(timeOn, normalizedCallSign),
      ...(force ? { _force: true } : {}),
    };

    for (const field of settings.exchange) {
      contact[field.adif] = String(exchangeValue(field, values))
        .trim()
        .toUpperCase();
    }

    onLogContact?.(contact);
    if (serialAllocation?.required) {
      onSerialContactLogged?.();
    }
    clearEntryFields();
    return true;
  }

  function entryFields(values = exchangeValues) {
    return [
      { name: 'CALL', value: callSign, ref: callSignRef, editable: true },
      ...(settings?.exchange ?? []).map((field) => ({
        name: field.name,
        value: exchangeValue(field, values),
        ref: { current: exchangeInputRefs.current[field.name] },
        editable: fieldEditable(field),
      })),
    ];
  }

  function focusRelativeEditableField(
    currentFieldName,
    values = exchangeValues,
    { direction = 1, preferEmpty = false } = {},
  ) {
    const fields = entryFields(values);
    const editableFields = fields.filter((field) => field.editable);
    if (editableFields.length <= 1) {
      return false;
    }

    const currentIndex = fields.findIndex(
      (field) => field.name === currentFieldName,
    );
    if (currentIndex < 0) return false;

    if (preferEmpty) {
      for (let step = 1; step <= fields.length; step += 1) {
        const nextIndex =
          (currentIndex + direction * step + fields.length) % fields.length;
        const nextField = fields[nextIndex];
        if (!nextField?.editable) continue;
        if (String(nextField.value).trim() !== '') continue;

        nextField.ref.current?.focus();
        return true;
      }
    }

    for (let step = 1; step <= fields.length; step += 1) {
      const nextIndex =
        (currentIndex + direction * step + fields.length) % fields.length;
      const nextField = fields[nextIndex];
      if (!nextField?.editable) continue;

      nextField.ref.current?.focus();
      return true;
    }

    return false;
  }

  function focusNextEditableField(currentFieldName, values = exchangeValues) {
    return focusRelativeEditableField(currentFieldName, values, {
      direction: 1,
      preferEmpty: false,
    });
  }

  function handleFieldTab(event, currentFieldName, values = exchangeValues) {
    if (event.key !== 'Tab') {
      return;
    }

    const focused = event.shiftKey
      ? focusRelativeEditableField(currentFieldName, values, {
          direction: -1,
          preferEmpty: false,
        })
      : focusRelativeEditableField(currentFieldName, values, {
          direction: 1,
          preferEmpty: true,
        });

    if (focused) {
      event.preventDefault();
    }
  }

  function exchangeFieldsValid(values = exchangeValues) {
    return (
      allRequiredFieldsFilled(values) &&
      callsignValidation().ok &&
      !firstInvalidExchangeField(values)
    );
  }

  function fieldFilledAndValid(fieldName, values = exchangeValues) {
    if (fieldName === 'CALL') {
      return callsignValidation().ok;
    }
    const field = (settings?.exchange ?? []).find(
      (item) => item.name === fieldName,
    );
    if (!field || !fieldEditable(field)) return false;
    const value = String(exchangeValue(field, values)).trim();
    return value !== '' && exchangeValidation(field, values).ok;
  }

  function nextInvalidExchangeFieldName(currentIndex, values = exchangeValues) {
    const exchangeFields = settings?.exchange ?? [];
    const totalFields = exchangeFields.length;
    if (totalFields === 0) return null;

    for (let step = 1; step <= totalFields; step += 1) {
      const nextIndex = (currentIndex + step) % totalFields;
      const field = exchangeFields[nextIndex];
      if (!field || !fieldEditable(field)) continue;

      const value = String(exchangeValue(field, values)).trim();
      if (value === '' || !exchangeValidation(field, values).ok) {
        return field.name;
      }
    }

    return null;
  }

  function currentEsmAction(values = exchangeValues) {
    return esmEnterAction({
      esmEnabled,
      operatingMode,
      callsign: callSign,
      exchangeValid: exchangeFieldsValid(values),
      exchangeSentCallsign: esmExchangeSentCallsign,
      runCallsignAttempt: esmRunCallsignAttempt,
    });
  }

  function handleEsmEnter(event, currentFieldName) {
    if (event.key !== 'Enter' || !esmEnabled) {
      return false;
    }

    event.preventDefault();

    if (
      !event.altKey &&
      currentFieldName === 'CALL' &&
      modeIsCw(radioMode) &&
      callsignHasQuery(callSign)
    ) {
      onSendCwText?.({
        request_id: createMessageRequestId(),
        text: callSign,
      });
      return true;
    }

    if (
      !event.altKey &&
      currentFieldName === 'CALL' &&
      shouldBlockEsmCallEnter(callSign, callsignValidation().ok)
    ) {
      return true;
    }

    const activeExchangeValues =
      currentFieldName === 'CALL'
        ? requestPreviousContactAutofill().values
        : exchangeValues;

    if (event.altKey) {
      logContact(true, activeExchangeValues);
      return true;
    }

    const esmAction = currentEsmAction(activeExchangeValues);
    if (
      modeIsCw(radioMode) &&
      operatingMode === 'Run' &&
      esmAction.correctionText
    ) {
      onSendCwText?.({
        request_id: createMessageRequestId(),
        text: esmAction.correctionText,
      });
    }
    sendEsmKeys(esmAction.keys, activeExchangeValues);
    setEsmRunCallsignAttempt(esmAction.nextRunCallsignAttempt);
    setEsmExchangeSentCallsign(esmAction.nextExchangeSentCallsign);

    if (esmAction.shouldLog) {
      logContact(false, activeExchangeValues);
      return true;
    }

    if (fieldFilledAndValid(currentFieldName, activeExchangeValues)) {
      focusNextEditableField(currentFieldName);
    }

    return true;
  }

  function handleCallsignKeyDown(event) {
    const value = callSign.trim();

    if (event.key === 'Tab') {
      const autofillResult = requestPreviousContactAutofill();
      handleFieldTab(event, 'CALL', autofillResult.values);
      return;
    }

    if (event.key === 'Enter' && isFrequencyInput(value)) {
      event.preventDefault();
      onSetRadioFrequency?.(Math.round(Number.parseFloat(value) * HZ_PER_KHZ));
      setCallSign('');
      pendingPreviousContactAutofillRef.current = '';
      callsignFrequencyBaselineRef.current = null;
      pendingBandMapTuneFrequencyRef.current = null;
      clearEsmState();
      return;
    }

    const typedMode = typedModeFromCallsignInput(value, settings);
    if (event.key === 'Enter' && typedMode) {
      event.preventDefault();
      onSetRadioMode?.(typedMode);
      setCallSign('');
      pendingPreviousContactAutofillRef.current = '';
      callsignFrequencyBaselineRef.current = null;
      pendingBandMapTuneFrequencyRef.current = null;
      clearEsmState();
      return;
    }

    if (event.key === 'Enter' && esmEnabled) {
      const autofillResult = previousContactExchangeAutofill({
        settings,
        contacts,
        callsign: value,
        exchangeValues,
        radioMode,
        contestParams: log?.contest_params ?? {},
      });

      if (
        shouldAdvanceFromCallsignAutofill({
          esmEnabled,
          autofillResult,
          hasEditableExchangeField: (settings?.exchange ?? []).some(
            fieldEditable,
          ),
        })
      ) {
        requestPreviousContactAutofill();
        if (focusNextEditableField('CALL')) {
          event.preventDefault();
          return;
        }
      }
    }

    if (handleEsmEnter(event, 'CALL')) {
      return;
    }

    if (event.key === 'Enter' && exchangeFieldsValid()) {
      event.preventDefault();
      logContact(false);
    }
  }

  function handleBandChange(event) {
    const selectedBand = bandByMeters(Number.parseInt(event.target.value, 10));

    if (selectedBand) {
      onSetRadioFrequency?.(selectedBand.lowerHz);
      if (isSelectableMode(radioMode)) {
        onSetRadioMode?.(radioMode);
      }
    }
  }

  function handleExchangeKeyDown(event, index) {
    const currentField = settings.exchange[index];
    const currentFieldName = currentField?.name;

    if (
      event.key === 'Enter' &&
      esmEnabled &&
      currentField &&
      currentField.fixed !== true
    ) {
      if (event.altKey) {
        event.preventDefault();
        logContact(true);
        return;
      }

      if (fieldFilledAndValid(currentFieldName)) {
        const nextInvalidFieldName = nextInvalidExchangeFieldName(index);
        if (nextInvalidFieldName) {
          event.preventDefault();
          exchangeInputRefs.current[nextInvalidFieldName]?.focus();
          return;
        }
      }
    }

    if (handleEsmEnter(event, currentFieldName)) {
      return;
    }

    if (event.key === 'Enter' && exchangeFieldsValid()) {
      event.preventDefault();
      logContact(false);
      return;
    }

    handleFieldTab(event, currentFieldName);
  }

  function handleCwWpmChange(event) {
    const wpm = Number.parseInt(event.target.value, 10);
    if (!Number.isFinite(wpm)) {
      setCwWpm(DEFAULT_CW_WPM);
      return;
    }
    setCwWpm(Math.min(Math.max(wpm, CW_WPM_MIN), CW_WPM_MAX));
  }

  function handleSpotIt() {
    const normalizedCallsign = currentCallsign();
    let spotCallsign = normalizedCallsign;
    let frequencyHz = radioFrequencyHz;

    if (!spotCallsign) {
      spotCallsign = String(lastContact?.CALL ?? lastContact?.Call ?? '')
        .trim()
        .toUpperCase();
      frequencyHz = normalizedContactFrequencyHz(
        lastContact?.FREQ ?? lastContact?.Freq,
      );
    }

    if (!spotCallsign || !frequencyHz) return;
    const comment = window.prompt('Spot comment', '');
    if (comment === null) return;

    onSendDxClusterSpot?.({
      frequency_hz: frequencyHz,
      call: spotCallsign,
      comment: String(comment ?? '').trim(),
    });
  }

  function handleQrzClick() {
    const normalizedCallsign = callSign.trim().toUpperCase();
    const qrzUrl = normalizedCallsign
      ? `https://www.qrz.com/db/${normalizedCallsign}`
      : 'https://www.qrz.com';

    window.open(qrzUrl, '_blank', 'noopener,noreferrer');
  }

  function openHelpWindow() {
    window.open('/help/index.html', '_blank', 'noopener,noreferrer');
  }

  const esmNextAction = currentEsmAction();
  const esmHighlightedKeys = esmEnabled
    ? esmNextAction.shouldLog && operatingMode === 'Run'
      ? [...new Set([...esmNextAction.keys, 'F3'])]
      : esmNextAction.keys
    : [];
  const highlightLogIt = esmEnabled && esmNextAction.shouldLog;

  return (
    <div className="window">
      <div className="title-bar logger-title-bar">
        <span>
          Log73 | Log: {log?.name ?? 'Loading...'} | Radio:{' '}
          {radio?.name ?? 'Loading...'} | Contest:{' '}
          {settings?.contest ?? 'Loading...'} | Mode: {radioMode}, Freq:{' '}
          {formatFrequency(radioFrequencyHz)}
        </span>
        <div className="logger-title-right">
          {loadingStatus ? (
            <span className="logger-loading-status">{loadingStatus}</span>
          ) : null}
          <div className="logger-title-actions">
            <button className="title-button" onClick={onExit}>
              Exit Logger
            </button>
            <button
              className="title-button title-help-button"
              type="button"
              aria-label="Open help"
              title="Open help"
              onClick={openHelpWindow}
            >
              ?
            </button>
          </div>
        </div>
      </div>
      <RadioControls
        operatingMode={operatingMode}
        setOperatingMode={setOperatingMode}
        currentBandAllowed={currentBandAllowed}
        currentBandValue={currentBandValue}
        bandOptions={bandOptions}
        currentBand={currentBand}
        handleBandChange={handleBandChange}
        radioMode={radioMode}
        onSetRadioMode={onSetRadioMode}
        esmEnabled={esmEnabled}
        onSetEsmEnabled={setEsmEnabled}
        cwWpm={cwWpm}
        cwWpmMin={CW_WPM_MIN}
        cwWpmMax={CW_WPM_MAX}
        handleCwWpmChange={handleCwWpmChange}
        bandMapEnabled={bandMapEnabled}
        onSetBandMapEnabled={onSetBandMapEnabled}
        backendSocketStatus={backendSocketStatus}
        catStatus={catStatus}
      />
      <EntryFields
        settings={settings}
        radioMode={radioMode}
        callSignRef={callSignRef}
        callSign={callSign}
        dxccLabel={currentDxccLabel}
        dupeAlertText={currentDupeAlertText}
        handleCallsignChange={handleCallsignChange}
        handleCallsignKeyDown={handleCallsignKeyDown}
        setActiveCompletionField={setActiveCompletionField}
        exchangeValue={exchangeValue}
        exchangeInputRefs={exchangeInputRefs}
        updateExchangeField={updateExchangeField}
        handleExchangeKeyDown={handleExchangeKeyDown}
      />
      {serialAllocation?.message ? (
        <div className="serial-allocation-status" aria-live="polite">
          {serialAllocation.message}
        </div>
      ) : null}
      <textarea
        className="supercheckpartial-box"
        rows="3"
        readOnly
        tabIndex={-1}
        aria-label="Completion matches"
        value={completionMatches.join(' ')}
      />
      {modeIsCw(radioMode) && isCwTextDialogOpen ? (
        <div className="cw-text-dialog-overlay" onClick={closeCwTextDialog}>
          <div
            className="cw-text-dialog"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="cw-text-dialog-header">
              <strong>CW Text</strong>
              <button
                className="title-button"
                type="button"
                aria-label="Close CW text dialog"
                onClick={closeCwTextDialog}
              >
                ×
              </button>
            </div>
            <div className="cw-text-dialog-body">
              <div className="cw-text-dialog-sent" aria-live="polite">
                {cwTextCommittedWords.join(' ')}
              </div>
              <input
                ref={cwTextInputRef}
                className="cw-text-dialog-input"
                type="text"
                value={cwTextCurrentWord}
                onChange={handleCwTextInputChange}
                onKeyDown={handleCwTextInputKeyDown}
                spellCheck={false}
                autoComplete="off"
                autoCorrect="off"
                autoCapitalize="off"
              />
            </div>
          </div>
        </div>
      ) : null}
      <FunctionKeys
        activeMessageLabels={activeMessageLabels}
        activeMessageKeys={activeMessageKeys}
        sendMessageKey={sendMessageKey}
        stationCallsign={stationCallsign}
        messageModeKey={messageModeKey}
        repeatRunF1={repeatRunF1}
        setRepeatRunF1={setRepeatRunF1}
        esmNextKeys={esmHighlightedKeys}
      />
      <CommandButtons
        stopMessageSending={stopMessageSending}
        clearEntryFields={clearEntryFields}
        logContact={logContact}
        onRescore={onRescore}
        isRescoreLoading={isRescoreLoading}
        disableRescore={isContextLoading || contactsLoadState !== 'idle'}
        handleQrzClick={handleQrzClick}
        handleMark={markCurrentFrequency}
        handleStore={storeCurrentBandMapSpot}
        handleSpotIt={handleSpotIt}
        highlightLogIt={highlightLogIt}
        disableLogIt={Boolean(serialBlockMessage)}
        logItTitle={serialBlockMessage || undefined}
      />
      <StatusBar
        stationCallsign={stationCallsign}
        operatorCallsign={operatorCallsign}
        scoreSummary={scoreSummary}
      />
    </div>
  );
}

export default MainWindow;
