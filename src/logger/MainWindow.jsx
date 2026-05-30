import React, { useEffect, useRef, useState } from 'react';
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
import { dxccLabel, lookupDxcc } from '../domain/dxcc';
import { dupeAlertText } from '../domain/dupes';
import { validateExchangeField } from '../domain/validation';
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
  stationCallsign,
  operatorCallsign,
  radioState,
  backendSocketStatus,
  catStatus,
  messageLabels,
  messageSentEvent,
  sessionId,
  logId,
  onSetRadioFrequency,
  onSetRadioMode,
  onSendMessage,
  onSendCwText,
  onStopCw,
  onSetCwWpm,
  onLogContact,
  onDebouncedCallsignChange,
  onRescore,
  isRescoreLoading,
  scoreSummary,
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
          setDxccData(result?.ok ? result.dxcc ?? null : null);
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
      callsignCompletionMatches(supercheckpartialCallsigns, query),
    );
  }, [
    activeCompletionField,
    debouncedCallSign,
    supercheckpartialCallsigns,
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

  function currentMessageFields() {
    const fields = {
      STATION_CALLSIGN: stationCallsign,
      CALL: callSign.trim().toUpperCase(),
    };

    for (const field of settings?.exchange ?? []) {
      fields[field.adif] = String(
        exchangeValues[field.name] ??
          fieldDefault(field, radioMode, log?.contest_params ?? {}),
      )
        .trim()
        .toUpperCase();
    }

    fields.EXCH = buildSentExchange(
      settings,
      exchangeValues,
      radioMode,
      log?.contest_params ?? {},
    );

    return fields;
  }

  function currentContactFields() {
    const contact = {
      CALL: callSign.trim().toUpperCase(),
      BAND: currentBand?.name ?? '',
      FREQ: radioFrequencyHz,
      MODE: adifModeForLoggerMode(radioMode),
    };

    for (const field of settings?.exchange ?? []) {
      contact[field.adif] = String(exchangeValue(field)).trim().toUpperCase();
    }

    return contact;
  }

  function currentCallsign() {
    return callSign.trim().toUpperCase();
  }

  function clearEsmState() {
    setEsmRunCallsignAttempt('');
    setEsmExchangeSentCallsign('');
  }

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

  function markMessageKeyActive(requestId, key) {
    activeMessageRequestsRef.current.set(requestId, key);
    setActiveMessageKeys((current) => new Set(current).add(key));
    const timeoutMs = cwActiveTimeoutMs(radio?.cw_keyer_type);
    const timeoutId = window.setTimeout(
      () => clearMessageRequest(requestId),
      timeoutMs,
    );
    activeMessageTimeoutsRef.current.set(requestId, timeoutId);
  }

  function clearMessageRequest(requestId) {
    const key = activeMessageRequestsRef.current.get(requestId);
    if (!key) return;
    activeMessageRequestsRef.current.delete(requestId);
    const timeoutId = activeMessageTimeoutsRef.current.get(requestId);
    if (timeoutId !== undefined) {
      window.clearTimeout(timeoutId);
      activeMessageTimeoutsRef.current.delete(requestId);
    }
    setActiveMessageKeys((current) => {
      const stillActive = [...activeMessageRequestsRef.current.values()].includes(
        key,
      );
      if (stillActive) return current;
      const next = new Set(current);
      next.delete(key);
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

  function sendSingleMessageKey(key, mode = messageModeKey) {
    const button = (messageLabels?.[mode] ?? DEFAULT_MESSAGE_LABELS[mode]).find(
      (label) => label.key === key,
    );
    if (isEmptyMessageButton(button)) return null;
    const requestId = createMessageRequestId();
    markMessageKeyActive(requestId, key);
    onSendMessage?.({
      request_id: requestId,
      mode,
      key,
      fields: currentMessageFields(),
    });
    return requestId;
  }

  repeatSendRunF1Ref.current = () => {
    repeatRequestIdRef.current = sendSingleMessageKey('F1', 'run');
  };
  callSignValueRef.current = callSign;

  function sendMessageKey(key) {
    const shouldRepeat = messageModeKey === 'run' && key === 'F1' && repeatRunF1;
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
    if (messageSentEvent?.requestId) clearMessageRequest(messageSentEvent.requestId);
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
    const word = cwTextCurrentWord.trim();
    if (!word) return;

    onSendCwText?.({
      request_id: createMessageRequestId(),
      text: sendTrailingSpace ? `${word} ` : word,
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

  function sendEsmKeys(keys) {
    stopRepeat();
    for (const key of keys) {
      const requestId = sendSingleMessageKey(key);
      if (!requestId) continue;
      if (key === 'F2') {
        markEsmExchangeSentForCurrentCallsign();
      }
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
      if (event.key === 'Escape') {
        event.preventDefault();
        stopMessageSending();
        return;
      }
      if (event.key === 'PageUp') {
        event.preventDefault();
        setCwWpm((current) => nextCwWpm(current, 1));
        return;
      }
      if (event.key === 'PageDown') {
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
    if (normalizedCallsign !== esmExchangeSentCallsign) {
      setEsmExchangeSentCallsign('');
    }
    setCallSign(sanitizedCallsign);
    callSignEditedAtRef.current = new Date();
  }

  function exchangeValue(field) {
    return (
      exchangeValues[field.name] ??
      fieldDefault(field, radioMode, log?.contest_params ?? {})
    );
  }

  function exchangeValidation(field) {
    return validateExchangeField(field, exchangeValue(field), radioMode);
  }

  function firstInvalidExchangeField() {
    return (settings?.exchange ?? []).find(
      (field) => !exchangeValidation(field).ok,
    );
  }

  function allRequiredFieldsFilled() {
    return (
      Boolean(settings?.exchange) &&
      callSign.trim() !== '' &&
      settings.exchange.every(
        (field) => String(exchangeValue(field)).trim() !== '',
      )
    );
  }

  function canLogContact(force = false) {
    return allRequiredFieldsFilled() && (force || !firstInvalidExchangeField());
  }

  function resetEntryFields() {
    setCallSign('');
    setExchangeValues(
      exchangeDefaults(settings, radioMode, log?.contest_params ?? {}),
    );
    clearEsmState();
    callSignEditedAtRef.current = new Date();
    callSignRef.current?.focus();
  }

  function logContact(force = false) {
    if (!canLogContact(force)) {
      const invalidField = firstInvalidExchangeField();
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
    };

    for (const field of settings.exchange) {
      contact[field.adif] = String(exchangeValue(field)).trim().toUpperCase();
    }

    onLogContact?.(contact);
    resetEntryFields();
    return true;
  }

  function focusNextEmptyField(currentFieldName) {
    const fields = [
      { name: 'CALL', value: callSign, ref: callSignRef, editable: true },
      ...(settings?.exchange ?? []).map((field) => ({
        name: field.name,
        value: exchangeValues[field.name] ?? '',
        ref: { current: exchangeInputRefs.current[field.name] },
        editable: field.fixed !== true,
      })),
    ];
    const currentIndex = fields.findIndex(
      (field) => field.name === currentFieldName,
    );
    const nextEmptyField = fields
      .slice(currentIndex + 1)
      .find((field) => field.editable && String(field.value).trim() === '');

    if (!nextEmptyField) {
      return false;
    }

    nextEmptyField.ref.current?.focus();
    return true;
  }

  function focusNextEditableField(currentFieldName) {
    const fields = [
      { name: 'CALL', ref: callSignRef, editable: true },
      ...(settings?.exchange ?? []).map((field) => ({
        name: field.name,
        ref: { current: exchangeInputRefs.current[field.name] },
        editable: field.fixed !== true,
      })),
    ];
    const currentIndex = fields.findIndex(
      (field) => field.name === currentFieldName,
    );
    const nextEditableField = fields
      .slice(currentIndex + 1)
      .find((field) => field.editable);

    if (!nextEditableField) {
      return false;
    }

    nextEditableField.ref.current?.focus();
    return true;
  }

  function handleFieldTab(event, currentFieldName) {
    if (event.key !== 'Tab' || event.shiftKey) {
      return;
    }

    if (focusNextEmptyField(currentFieldName)) {
      event.preventDefault();
    }
  }

  function exchangeFieldsValid() {
    return allRequiredFieldsFilled() && !firstInvalidExchangeField();
  }

  function fieldFilledAndValid(fieldName) {
    if (fieldName === 'CALL') {
      return callSign.trim() !== '';
    }
    const field = (settings?.exchange ?? []).find(
      (item) => item.name === fieldName,
    );
    if (!field || field.fixed === true) return false;
    const value = String(exchangeValue(field)).trim();
    return value !== '' && exchangeValidation(field).ok;
  }

  function nextInvalidExchangeFieldName(currentIndex) {
    const exchangeFields = settings?.exchange ?? [];
    const totalFields = exchangeFields.length;
    if (totalFields === 0) return null;

    for (let step = 1; step <= totalFields; step += 1) {
      const nextIndex = (currentIndex + step) % totalFields;
      const field = exchangeFields[nextIndex];
      if (!field || field.fixed === true) continue;

      const value = String(exchangeValue(field)).trim();
      if (value === '' || !exchangeValidation(field).ok) {
        return field.name;
      }
    }

    return null;
  }

  function currentEsmAction() {
    return esmEnterAction({
      esmEnabled,
      operatingMode,
      callsign: callSign,
      exchangeValid: exchangeFieldsValid(),
      exchangeSentCallsign: esmExchangeSentCallsign,
      runCallsignAttempt: esmRunCallsignAttempt,
    });
  }

  function handleEsmEnter(event, currentFieldName) {
    if (event.key !== 'Enter' || !esmEnabled) {
      return false;
    }

    event.preventDefault();

    if (event.altKey) {
      logContact(false);
      return true;
    }

    const esmAction = currentEsmAction();
    sendEsmKeys(esmAction.keys);
    setEsmRunCallsignAttempt(esmAction.nextRunCallsignAttempt);
    setEsmExchangeSentCallsign(esmAction.nextExchangeSentCallsign);

    if (esmAction.shouldLog) {
      logContact(false);
      return true;
    }

    if (fieldFilledAndValid(currentFieldName)) {
      focusNextEditableField(currentFieldName);
    }

    return true;
  }

  function handleCallsignKeyDown(event) {
    const value = callSign.trim();

    if (event.key === 'Tab') {
      handleFieldTab(event, 'CALL');
      return;
    }

    if (event.key === 'Enter' && isFrequencyInput(value)) {
      event.preventDefault();
      onSetRadioFrequency?.(Math.round(Number.parseFloat(value) * HZ_PER_KHZ));
      setCallSign('');
      clearEsmState();
      return;
    }

    const typedMode = typedModeFromCallsignInput(value, settings);
    if (event.key === 'Enter' && typedMode) {
      event.preventDefault();
      onSetRadioMode?.(typedMode);
      setCallSign('');
      clearEsmState();
      return;
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
        logContact(false);
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

  function handleQrzClick() {
    const normalizedCallsign = callSign.trim().toUpperCase();
    const qrzUrl = normalizedCallsign
      ? `https://www.qrz.com/db/${normalizedCallsign}`
      : 'https://www.qrz.com';

    window.open(qrzUrl, '_blank', 'noopener,noreferrer');
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
        {loadingStatus ? (
          <span className="logger-loading-status">{loadingStatus}</span>
        ) : null}
        <button className="title-button" onClick={onExit}>
          Exit Logger
        </button>
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
        resetEntryFields={resetEntryFields}
        logContact={logContact}
        onRescore={onRescore}
        isRescoreLoading={isRescoreLoading}
        disableRescore={isContextLoading || contactsLoadState !== 'idle'}
        handleQrzClick={handleQrzClick}
        highlightLogIt={highlightLogIt}
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
