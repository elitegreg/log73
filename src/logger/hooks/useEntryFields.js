import { useEffect, useRef, useState } from 'react';
import {
  CALLSIGN_LOOKUP_DEBOUNCE_MS,
  exchangeDefaults,
  callsignClearThresholdHz,
  shouldAdvanceFromCallsignAutofill,
} from '../mainWindowHelpers';
import {
  fieldDefault,
  sanitizeCallsign,
  sanitizeExchangeValue,
} from '../../domain/contactFields';
import { previousContactExchangeAutofill } from '../mainWindowHelpers';

export function useEntryFields({
  settings,
  radioMode,
  log,
  serialAllocation,
  bandMapSelection,
  radioFrequencyHz,
  contacts,
}) {
  const [callSign, setCallSign] = useState('');
  const [debouncedCallSign, setDebouncedCallSign] = useState('');
  const [exchangeValues, setExchangeValues] = useState({});
  const [operatingMode, setOperatingMode] = useState('S&P');
  const callSignRef = useRef(null);
  const callsignSelectionRef = useRef(null);
  const exchangeInputRefs = useRef({});
  const callSignEditedAtRef = useRef(new Date());
  const callsignFrequencyBaselineRef = useRef(null);
  const pendingBandMapTuneFrequencyRef = useRef(null);
  const pendingPreviousContactAutofillRef = useRef('');
  const bandMapSelectionSequenceRef = useRef(null);

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
    const selection = callsignSelectionRef.current;
    const input = callSignRef.current;
    if (!selection || !input || document.activeElement !== input) return;

    const start = Math.min(selection.start, input.value.length);
    const end = Math.min(selection.end, input.value.length);
    input.setSelectionRange(start, end);
    callsignSelectionRef.current = null;
  }, [callSign]);

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
    callSignEditedAtRef.current = new Date();
  }, [callSign, radioFrequencyHz, radioMode]);

  function updateExchangeField(field, value) {
    setExchangeValues((current) => ({
      ...current,
      [field.name]: sanitizeExchangeValue(field, value, radioMode),
    }));
  }

  function handleCallsignChange(
    event,
    { stopRepeat, clearEsmState, esmRunCallsignAttempt },
  ) {
    stopRepeat();
    const { selectionStart, selectionEnd } = event.target;
    callsignSelectionRef.current = {
      start: selectionStart ?? event.target.value.length,
      end: selectionEnd ?? event.target.value.length,
    };
    const sanitizedCallsign = sanitizeCallsign(event.target.value);
    const normalizedCallsign = sanitizedCallsign.trim().toUpperCase();
    if (normalizedCallsign !== esmRunCallsignAttempt) {
      clearEsmState();
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

  function shouldAutofillAdvance(esmEnabled) {
    const autofillResult = previousContactExchangeAutofill({
      settings,
      contacts,
      callsign: callSign.trim(),
      exchangeValues,
      radioMode,
      contestParams: log?.contest_params ?? {},
    });

    return shouldAdvanceFromCallsignAutofill({
      esmEnabled,
      autofillResult,
      hasEditableExchangeField: (settings?.exchange ?? []).some(
        (field) => field.fixed !== true,
      ),
    });
  }

  return {
    callSign,
    setCallSign,
    debouncedCallSign,
    setDebouncedCallSign,
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
    handleCallsignChange,
    updateExchangeField,
    exchangeValue,
    currentCallsign,
    requestPreviousContactAutofill,
    shouldAutofillAdvance,
  };
}
