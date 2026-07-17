import React, { useEffect, useRef, useState } from 'react';
import { buildSentExchange, fieldDefault } from '../domain/contactFields';
import { dxccContinent } from '../domain/dxcc';
import { validateCallsign, validateExchangeField } from '../domain/validation';
import {
  CW_WPM_STORAGE_KEY,
  DEFAULT_MESSAGE_LABELS,
  DEFAULT_CW_WPM,
  CW_WPM_MIN,
  CW_WPM_MAX,
  DEFAULT_RADIO_FREQUENCY_HZ,
  HZ_PER_KHZ,
  EPOCH_MS_PER_SECOND,
  typedModeFromCallsignInput,
  exchangeDefaults,
  previousContactExchangeAutofill,
  formatFrequency,
  isFrequencyInput,
  adifModeForLoggerMode,
  modeIsCw,
  modeIsPhone,
  esmEnterAction,
  bandForFrequency,
  bandByName,
  createContactId,
  createMessageRequestId,
  callsignHasQuery,
  shouldBlockEsmCallEnter,
  normalizedContactFrequencyHz,
  shouldAdvanceFromCallsignAutofill,
  callsignClearThresholdHz,
  loggerFrequencyChangeAction,
} from './mainWindowHelpers';
import RadioControls from './components/RadioControls';
import EntryFields from './components/EntryFields';
import FunctionKeys from './components/FunctionKeys';
import CommandButtons from './components/CommandButtons';
import StatusBar from './components/StatusBar';
import { useBandControls } from './hooks/useBandControls';
import { useCompletions } from './hooks/useCompletions';
import { useCwTextDialog } from './hooks/useCwTextDialog';
import { useEntryFields } from './hooks/useEntryFields';
import { useEsm } from './hooks/useEsm';
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts';
import { useMessageSending } from './hooks/useMessageSending';

function dxccAdifNumber(dxccInfo) {
  const dxccNumber = Number(dxccInfo?.adif);
  return Number.isInteger(dxccNumber) && dxccNumber > 0 ? dxccNumber : null;
}

function dxccPrefix(dxccInfo) {
  const prefix = String(dxccInfo?.primary_prefix ?? '').trim();
  return prefix || null;
}

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
  supercheckpartialUpdate,
  onSetBandMapEnabled,
  onActivateBandMapSpot,
  onStoreCqFrequency,
  onMarkFrequency,
  onStoreBandMapSpot,
  onRegisterBandMapActivateClear,
  onSetRadioFrequency,
  onSetRadioMode,
  onClearRit,
  onIncrementRit,
  onDecrementRit,
  onSendMessage,
  onSendCwText,
  onSendDxClusterSpot,
  onStopKeying,
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
  const radioMode = radioState?.mode ?? 'CW';
  const radioFrequencyHz =
    radioState?.frequency_hz ?? DEFAULT_RADIO_FREQUENCY_HZ;
  const {
    callSign,
    setCallSign,
    debouncedCallSign,
    exchangeValues,
    setExchangeValues,
    operatingMode,
    setOperatingMode,
    callSignRef,
    exchangeInputRefs,
    callSignEditedAtRef,
    callsignFrequencyBaselineRef,
    pendingBandMapTuneFrequencyRef,
    pendingPreviousContactAutofillRef,
    handleCallsignChange: handleEntryCallsignChange,
    updateExchangeField,
    exchangeValue,
    currentCallsign: entryCurrentCallsign,
    requestPreviousContactAutofill: entryRequestPreviousContactAutofill,
  } = useEntryFields({
    settings,
    radioMode,
    log,
    serialAllocation,
    bandMapSelection,
    radioFrequencyHz,
    contacts,
  });
  const {
    esmEnabled,
    setEsmEnabled,
    esmRunCallsignAttempt,
    setEsmRunCallsignAttempt,
    esmExchangeSentCallsign,
    setEsmExchangeSentCallsign,
  } = useEsm();
  const [activeCompletionField, setActiveCompletionField] = useState(null);
  const [cwWpm, setCwWpm] = useState(() => {
    const storedWpm = Number.parseInt(
      localStorage.getItem(CW_WPM_STORAGE_KEY) ?? '',
      10,
    );
    return Number.isFinite(storedWpm) ? storedWpm : DEFAULT_CW_WPM;
  });
  const setCwWpmRef = useRef(onSetCwWpm);
  const previousRadioFrequencyHzRef = useRef(null);
  const allowedBands = settings?.allowed_bands ?? [];
  const bandCatalog = settings?.band_catalog ?? [];
  const currentBand = bandForFrequency(radioFrequencyHz, bandCatalog);
  const currentBandValue = currentBand ? currentBand.name : 'unknown';
  const currentBandAllowed = currentBand
    ? allowedBands.includes(currentBand.name)
    : false;
  const bandOptions = allowedBands
    .map((bandName) => bandByName(bandCatalog, bandName))
    .filter(Boolean);
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
    !bandOptions.some((band) => band.name === currentBand.name)
  ) {
    bandOptions.push(currentBand);
  }

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
    onDebouncedCallsignChange?.(debouncedCallSign.trim().toUpperCase());
  }, [debouncedCallSign, onDebouncedCallsignChange]);

  useEffect(() => {
    const previousFrequencyHz = previousRadioFrequencyHzRef.current;
    previousRadioFrequencyHzRef.current = radioFrequencyHz;
    const action = loggerFrequencyChangeAction({
      previousFrequencyHz,
      nextFrequencyHz: radioFrequencyHz,
      thresholdHz: callsignClearThresholdHz(radioMode),
      pendingBandMapTuneFrequencyHz: pendingBandMapTuneFrequencyRef.current,
    });
    if (action === 'clear-pending-bandmap-tune') {
      pendingBandMapTuneFrequencyRef.current = null;
      return;
    }
    if (action !== 'clear-logger') return;
    if (operatingMode === 'Run') {
      setOperatingMode('S&P');
    }
    clearEntryFields();
  }, [operatingMode, radioFrequencyHz, radioMode]);

  const messageModeKey = operatingMode === 'Run' ? 'run' : 's&p';
  const modeMessageLabels = modeIsPhone(radioMode)
    ? (messageLabels?.voice ?? null)
    : (messageLabels?.cw ?? messageLabels);
  const activeMessageLabels =
    modeMessageLabels?.[messageModeKey] ??
    DEFAULT_MESSAGE_LABELS[messageModeKey];

  function currentMessageFields(values = exchangeValues) {
    const fields = {
      STATION_CALLSIGN: stationCallsign,
      OPERATOR: operatorCallsign,
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
    return entryCurrentCallsign();
  }

  function requestPreviousContactAutofill(values = exchangeValues) {
    return entryRequestPreviousContactAutofill(values);
  }

  const {
    completionMatches,
    currentDxccInfo,
    currentDxccLabel,
    currentDupeAlertText,
  } = useCompletions({
    bandMapSpotStore,
    contacts,
    settings,
    activeCompletionField,
    setActiveCompletionField,
    debouncedCallSign,
    callSign,
    exchangeValues,
    radioMode,
    log,
    currentContactFields,
    supercheckpartialUpdate,
  });

  const {
    isCwTextDialogOpen,
    cwTextCommittedWords,
    cwTextCurrentWord,
    cwTextInputRef,
    openCwTextDialog,
    closeCwTextDialog,
    handleCwTextInputChange,
    handleCwTextInputKeyDown,
  } = useCwTextDialog({
    radioMode,
    onSendCwText,
    callSignRef,
  });

  const {
    storeCurrentCqFrequency,
    jumpToLastCqFrequency,
    markCurrentFrequency,
    storeCurrentBandMapSpot,
    activateBandMapSpot,
    clearRitIfEnabled,
    tuneByIncrement,
    shiftBand,
    handleBandChange,
  } = useBandControls({
    operatingMode,
    radio,
    radioMode,
    radioFrequencyHz,
    currentBand,
    bandOptions,
    bandMapSpotStore,
    currentCallsign,
    currentExchangeFields: currentContactFields,
    onStoreCqFrequency,
    onMarkFrequency,
    onStoreBandMapSpot,
    onActivateBandMapSpot,
    onSetRadioFrequency,
    onSetRadioMode,
    onClearRit,
    onIncrementRit,
    onDecrementRit,
  });

  useEffect(() => {
    if (operatingMode === 'Run') {
      storeCurrentCqFrequency();
    }
  }, [operatingMode, storeCurrentCqFrequency]);

  function clearEsmState() {
    setEsmRunCallsignAttempt('');
    setEsmExchangeSentCallsign('');
  }

  function markEsmExchangeSentForCurrentCallsign() {
    const normalizedCallsign = currentCallsign();
    if (!normalizedCallsign) return;
    setEsmExchangeSentCallsign(normalizedCallsign);
  }

  const {
    repeatRunF1,
    setRepeatRunF1,
    activeMessageKeys,
    sendMessageKey,
    sendEsmKeys,
    stopMessageSending,
    stopRepeat,
  } = useMessageSending({
    radio,
    radioMode,
    messageLabels,
    messageModeKey,
    messageSentEvent,
    currentMessageFields: (values = exchangeValues) =>
      currentMessageFields(values),
    currentCallsign: () => callSign,
    storeCurrentCqFrequency,
    markEsmExchangeSentForCurrentCallsign,
    clearEntryFields,
    onSendMessage,
    onStopKeying,
  });

  useKeyboardShortcuts({
    radioMode,
    bandMapSpotStore,
    radioFrequencyHz,
    isCwTextDialogOpen,
    openCwTextDialog,
    closeCwTextDialog,
    jumpToLastCqFrequency,
    markCurrentFrequency,
    storeCurrentBandMapSpot,
    handleSpotIt,
    activateBandMapSpot,
    shiftBand,
    tuneByIncrement,
    setCwWpm,
    sendMessageKey,
    stopMessageSending,
  });

  function handleCallsignChange(event) {
    handleEntryCallsignChange(event, {
      stopRepeat,
      clearEsmState,
      esmRunCallsignAttempt,
    });
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

  useEffect(() => {
    onRegisterBandMapActivateClear?.(clearEntryFields);
    return () => onRegisterBandMapActivateClear?.(null);
  }, [onRegisterBandMapActivateClear, clearRitIfEnabled, radioMode, settings, log]);

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
    const dxccNumber = dxccAdifNumber(currentDxccInfo);
    const prefix = dxccPrefix(currentDxccInfo);
    const continent = dxccContinent(currentDxccInfo);
    const contact = {
      meta: {
        status: 'Pending',
        sessionId,
        logId,
        clientId: createContactId(timeOn, normalizedCallSign),
        ...(prefix === null ? {} : { DXCC_PREFIX: prefix }),
        ...(force ? { force: true } : {}),
      },
      adif: {
        QSO_DATE_TIME_ON: Math.floor(timeOn.getTime() / EPOCH_MS_PER_SECOND),
        STATION_CALLSIGN: stationCallsign,
        OPERATOR: operatorCallsign,
        CONTEST_ID: settings.contest,
        CALL: normalizedCallSign,
        BAND: currentBand?.name ?? '',
        FREQ: radioFrequencyHz,
        MODE: adifModeForLoggerMode(radioMode),
        ...(dxccNumber === null ? {} : { DXCC: dxccNumber }),
        ...(continent === null ? {} : { CONT: continent }),
      },
    };

    for (const field of settings.exchange) {
      contact.adif[field.adif] = String(exchangeValue(field, values))
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
        text: `${esmAction.correctionText} `,
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
          operatingMode,
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
    let exchangeFields = currentContactFields();

    if (!spotCallsign) {
      spotCallsign = String(lastContact?.adif?.CALL ?? '')
        .trim()
        .toUpperCase();
      frequencyHz = normalizedContactFrequencyHz(
        lastContact?.adif?.FREQ ?? lastContact?.FREQ ?? lastContact?.Freq,
      );
      exchangeFields = { ...(lastContact?.adif ?? {}) };
    }

    if (!spotCallsign || !frequencyHz) return;
    const spotLabel = `Spot ${spotCallsign} @ ${formatFrequency(frequencyHz)} kHz`;
    const comment = window.prompt(spotLabel, '');
    if (comment === null) return;

    const normalizedComment = String(comment ?? '').trim();
    onSendDxClusterSpot?.({
      frequency_hz: frequencyHz,
      call: spotCallsign,
      comment: normalizedComment,
    });
    onStoreBandMapSpot?.({
      spot_type: 'local',
      frequency_hz: frequencyHz,
      call: spotCallsign,
      comment: normalizedComment,
      exchange_fields: exchangeFields,
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
