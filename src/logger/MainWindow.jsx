import React, { useEffect, useRef, useState } from 'react';
import {
  callsignCompletionMatches,
  exchangeCompletionMatches,
} from '../domain/completions';
import {
  fieldDefault,
  sanitizeCallsign,
  sanitizeExchangeValue,
} from '../domain/contactFields';
import { validateExchangeField } from '../domain/validation';
import { supercheckpartial } from '../lib/api';
import {
  CW_WPM_STORAGE_KEY,
  DEFAULT_CW_LABELS,
  DEFAULT_CW_WPM,
  CW_WPM_MIN,
  CW_WPM_MAX,
  DEFAULT_RADIO_FREQUENCY_HZ,
  SUPERCHECKPARTIAL_MIN_QUERY_LENGTH,
  CW_ACTIVE_TIMEOUT_WIKEYER_MS,
  CW_ACTIVE_TIMEOUT_NO_WIKEYER_MS,
  CW_REPEAT_DELAY_MS,
  FUNCTION_KEY_PATTERN,
  HZ_PER_KHZ,
  EPOCH_MS_PER_SECOND,
  exchangeDefaults,
  formatFrequency,
  isFrequencyInput,
  bandForFrequency,
  bandByMeters,
  createContactId,
  createCwRequestId,
  isEmptyCwButton,
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
  stationCallsign,
  operatorCallsign,
  radioState,
  backendSocketStatus,
  cwLabels,
  cwSentEvent,
  sessionId,
  logId,
  onSetRadioFrequency,
  onSetRadioMode,
  onSendCw,
  onStopCw,
  onSetCwWpm,
  onLogContact,
  onRescore,
  isRescoreLoading,
  scoreSummary,
  onExit,
}) {
  const [callSign, setCallSign] = useState('');
  const [exchangeValues, setExchangeValues] = useState({});
  const [operatingMode, setOperatingMode] = useState('S&P');
  const [repeatRunF1, setRepeatRunF1] = useState(false);
  const [activeCwKeys, setActiveCwKeys] = useState(() => new Set());
  const [activeCompletionField, setActiveCompletionField] = useState(null);
  const [supercheckpartialCallsigns, setSupercheckpartialCallsigns] = useState(
    [],
  );
  const [supercheckpartialMatches, setSupercheckpartialMatches] = useState([]);
  const [cwWpm, setCwWpm] = useState(() => {
    const storedWpm = Number.parseInt(
      localStorage.getItem(CW_WPM_STORAGE_KEY) ?? '',
      10,
    );
    return Number.isFinite(storedWpm) ? storedWpm : DEFAULT_CW_WPM;
  });
  const callSignRef = useRef(null);
  const setCwWpmRef = useRef(onSetCwWpm);
  const repeatActiveRef = useRef(false);
  const repeatRequestIdRef = useRef(null);
  const repeatTimeoutRef = useRef(null);
  const callSignValueRef = useRef('');
  const repeatSendRunF1Ref = useRef(() => {});
  const callsignSelectionRef = useRef(null);
  const activeCwRequestsRef = useRef(new Map());
  const activeCwTimeoutsRef = useRef(new Map());
  const exchangeInputRefs = useRef({});
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
  const loadingStatus =
    isContextLoading
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
    if (activeCompletionField !== 'CALL') {
      setSupercheckpartialMatches([]);
      return;
    }

    const query = callSign.trim().toUpperCase();
    if (query.length < SUPERCHECKPARTIAL_MIN_QUERY_LENGTH) {
      setSupercheckpartialMatches([]);
      return;
    }

    setSupercheckpartialMatches(
      callsignCompletionMatches(supercheckpartialCallsigns, query),
    );
  }, [activeCompletionField, callSign, supercheckpartialCallsigns]);

  const cwModeKey = operatingMode === 'Run' ? 'run' : 's&p';
  const activeCwLabels = cwLabels?.[cwModeKey] ?? DEFAULT_CW_LABELS[cwModeKey];
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

  function currentCwFields() {
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

    return fields;
  }

  function stopRepeat() {
    repeatActiveRef.current = false;
    repeatRequestIdRef.current = null;
    if (repeatTimeoutRef.current !== null) {
      window.clearTimeout(repeatTimeoutRef.current);
      repeatTimeoutRef.current = null;
    }
  }

  function markCwKeyActive(requestId, key) {
    activeCwRequestsRef.current.set(requestId, key);
    setActiveCwKeys((current) => new Set(current).add(key));
    const timeoutMs = radio?.winkeyer_enabled
      ? CW_ACTIVE_TIMEOUT_WIKEYER_MS
      : CW_ACTIVE_TIMEOUT_NO_WIKEYER_MS;
    const timeoutId = window.setTimeout(
      () => clearCwRequest(requestId),
      timeoutMs,
    );
    activeCwTimeoutsRef.current.set(requestId, timeoutId);
  }

  function clearCwRequest(requestId) {
    const key = activeCwRequestsRef.current.get(requestId);
    if (!key) return;
    activeCwRequestsRef.current.delete(requestId);
    const timeoutId = activeCwTimeoutsRef.current.get(requestId);
    if (timeoutId !== undefined) {
      window.clearTimeout(timeoutId);
      activeCwTimeoutsRef.current.delete(requestId);
    }
    setActiveCwKeys((current) => {
      const stillActive = [...activeCwRequestsRef.current.values()].includes(
        key,
      );
      if (stillActive) return current;
      const next = new Set(current);
      next.delete(key);
      return next;
    });
  }

  function sendSingleCwKey(key, mode = cwModeKey) {
    const button = (cwLabels?.[mode] ?? DEFAULT_CW_LABELS[mode]).find(
      (label) => label.key === key,
    );
    if (isEmptyCwButton(button)) return null;
    const requestId = createCwRequestId();
    markCwKeyActive(requestId, key);
    onSendCw?.({
      request_id: requestId,
      mode,
      key,
      fields: currentCwFields(),
    });
    return requestId;
  }

  repeatSendRunF1Ref.current = () => {
    repeatRequestIdRef.current = sendSingleCwKey('F1', 'run');
  };
  callSignValueRef.current = callSign;

  function sendCwKey(key) {
    const shouldRepeat = cwModeKey === 'run' && key === 'F1' && repeatRunF1;
    stopRepeat();
    const requestId = sendSingleCwKey(key);
    if (!requestId) return;

    if (shouldRepeat) {
      repeatActiveRef.current = true;
      repeatRequestIdRef.current = requestId;
    }

    if (cwModeKey === 's&p' && key === 'F1') {
      setOperatingMode('Run');
    }
  }

  function stopCwSending() {
    stopRepeat();
    onStopCw?.();
  }

  useEffect(
    () => () => {
      stopRepeat();
      for (const timeoutId of activeCwTimeoutsRef.current.values()) {
        window.clearTimeout(timeoutId);
      }
      activeCwTimeoutsRef.current.clear();
      activeCwRequestsRef.current.clear();
    },
    [],
  );

  useEffect(() => {
    if (cwSentEvent?.requestId) clearCwRequest(cwSentEvent.requestId);
    if (
      !repeatActiveRef.current ||
      !cwSentEvent?.requestId ||
      cwSentEvent.requestId !== repeatRequestIdRef.current
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
  }, [cwSentEvent]);

  useEffect(() => {
    function handleFunctionKey(event) {
      if (event.target?.closest?.('.log-window')) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        stopCwSending();
        return;
      }
      if (FUNCTION_KEY_PATTERN.test(event.key)) {
        event.preventDefault();
        sendCwKey(event.key);
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
    setCallSign(sanitizeCallsign(event.target.value));
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
      MODE: radioMode,
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

  function handleFieldTab(event, currentFieldName) {
    if (event.key !== 'Tab' || event.shiftKey) {
      return;
    }

    if (focusNextEmptyField(currentFieldName)) {
      event.preventDefault();
    }
  }

  function handleCallsignKeyDown(event) {
    const value = callSign.trim();
    const forceLog = event.ctrlKey && event.altKey;

    if (event.key === 'Tab') {
      handleFieldTab(event, 'CALL');
      return;
    }

    if (event.key === 'Enter' && isFrequencyInput(value)) {
      event.preventDefault();
      onSetRadioFrequency?.(Math.round(Number.parseFloat(value) * HZ_PER_KHZ));
      setCallSign('');
      return;
    }

    if (event.key === 'Enter' && allRequiredFieldsFilled()) {
      event.preventDefault();
      logContact(forceLog);
    }
  }

  function handleBandChange(event) {
    const selectedBand = bandByMeters(Number.parseInt(event.target.value, 10));

    if (selectedBand) {
      onSetRadioFrequency?.(selectedBand.lowerHz);
      onSetRadioMode?.(radioMode);
    }
  }

  function handleExchangeKeyDown(event, index) {
    if (event.key === 'Enter' && allRequiredFieldsFilled()) {
      event.preventDefault();
      logContact(event.ctrlKey && event.altKey);
      return;
    }

    handleFieldTab(event, settings.exchange[index]?.name);
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
        cwWpm={cwWpm}
        cwWpmMin={CW_WPM_MIN}
        cwWpmMax={CW_WPM_MAX}
        handleCwWpmChange={handleCwWpmChange}
        backendSocketStatus={backendSocketStatus}
      />
      <EntryFields
        settings={settings}
        radioMode={radioMode}
        callSignRef={callSignRef}
        callSign={callSign}
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
      <FunctionKeys
        activeCwLabels={activeCwLabels}
        activeCwKeys={activeCwKeys}
        sendCwKey={sendCwKey}
        stationCallsign={stationCallsign}
        cwModeKey={cwModeKey}
        repeatRunF1={repeatRunF1}
        setRepeatRunF1={setRepeatRunF1}
      />
      <CommandButtons
        stopCwSending={stopCwSending}
        resetEntryFields={resetEntryFields}
        logContact={logContact}
        onRescore={onRescore}
        isRescoreLoading={isRescoreLoading}
        disableRescore={isContextLoading || contactsLoadState !== 'idle'}
        handleQrzClick={handleQrzClick}
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
