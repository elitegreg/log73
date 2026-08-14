import { useEffect, useRef, useState } from 'react';
import {
  CALLSIGN_LOOKUP_DEBOUNCE_MS,
  exchangeDefaults,
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
  locked = false,
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
    if (locked) {
      setExchangeValues({});
      return;
    }
    setExchangeValues(
      exchangeDefaults(settings, radioMode, log?.contest_params ?? {}),
    );
  }, [settings, radioMode, log, locked]);

  useEffect(() => {
    if (locked) return;
    if (!serialAllocation?.required || !serialAllocation.fieldAdif) return;
    const serialField = (settings?.exchange ?? []).find(
      (field) =>
        field.direction === 'sent' && field.adif === serialAllocation.fieldAdif,
    );
    if (!serialField) return;
    setExchangeValues((currentValues) => ({
      ...currentValues,
      [serialField.id]:
        serialAllocation.current === null ||
        serialAllocation.current === undefined
          ? ''
          : String(serialAllocation.current),
    }));
  }, [settings, serialAllocation, locked]);

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
    if (locked) return;
    if (!bandMapSelection?.spot) return;
    if (bandMapSelectionSequenceRef.current === bandMapSelection.sequence)
      return;
    bandMapSelectionSequenceRef.current = bandMapSelection.sequence;
    const callsign = sanitizeCallsign(bandMapSelection.spot?.call_dx ?? '');
    setOperatingMode('S&P');
    setCallSign(callsign);
    const nextExchangeValues = { ...exchangeValues };
    const spotExchangeFields = bandMapSelection.spot?.exchange_fields ?? null;
    if (spotExchangeFields && settings?.exchange) {
      for (const field of settings.exchange) {
        const rawValue =
          spotExchangeFields?.[field.adif] ?? spotExchangeFields?.[field.label];
        if (rawValue === undefined || rawValue === null) continue;
        nextExchangeValues[field.id] = sanitizeExchangeValue(
          field,
          rawValue,
          radioMode,
        );
      }
      setExchangeValues(nextExchangeValues);
    }
    pendingPreviousContactAutofillRef.current = '';
    callsignFrequencyBaselineRef.current =
      Number(bandMapSelection.spot?.frequency_hz) || null;
    pendingBandMapTuneFrequencyRef.current =
      callsignFrequencyBaselineRef.current;
    callSignEditedAtRef.current = new Date();
    window.requestAnimationFrame(() => callSignRef.current?.focus());
  }, [bandMapSelection, exchangeValues, radioMode, settings, locked]);

  function updateExchangeField(field, value) {
    if (locked) return;
    setExchangeValues((current) => ({
      ...current,
      [field.id]: sanitizeExchangeValue(field, value, radioMode),
    }));
  }

  function handleCallsignChange(
    event,
    { stopRepeat, updateEsmStateAfterCallsignEdit },
  ) {
    stopRepeat();
    const { selectionStart, selectionEnd } = event.target;
    callsignSelectionRef.current = {
      start: selectionStart ?? event.target.value.length,
      end: selectionEnd ?? event.target.value.length,
    };
    const sanitizedCallsign = sanitizeCallsign(event.target.value);
    const normalizedCallsign = sanitizedCallsign.trim().toUpperCase();
    updateEsmStateAfterCallsignEdit(normalizedCallsign);
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
      values?.[field.id] ??
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

  function shouldAutofillAdvance(esmEnabled, operatingMode = 'S&P') {
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
      operatingMode,
      autofillResult,
      hasEditableExchangeField: (settings?.exchange ?? []).some(
        (field) =>
          field.fixed !== true &&
          !(
            field.direction === 'sent' &&
            String(field?.input?.kind).toUpperCase() === 'SERIAL'
          ),
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
