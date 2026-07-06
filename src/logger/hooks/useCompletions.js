import { useEffect, useMemo, useState } from 'react';
import {
  callsignCompletionMatches,
  exchangeCompletionMatches,
} from '../../domain/completions';
import { dxccLabel, lookupDxcc } from '../../domain/dxcc';
import { dupeAlertText } from '../../domain/dupes';
import { dxcc, supercheckpartial } from '../../lib/api';
import { SUPERCHECKPARTIAL_MIN_QUERY_LENGTH } from '../mainWindowHelpers';

export function useCompletions({
  bandMapSpotStore,
  contacts,
  settings,
  activeCompletionField,
  setActiveCompletionField,
  debouncedCallSign,
  callSign,
  exchangeValues,
  currentContactFields,
}) {
  const [supercheckpartialCallsigns, setSupercheckpartialCallsigns] = useState(
    [],
  );
  const [supercheckpartialMatches, setSupercheckpartialMatches] = useState([]);
  const [dxccData, setDxccData] = useState(null);

  useEffect(() => {
    let cancelled = false;
    supercheckpartial()
      .then((result) => {
        if (!cancelled) {
          const callsigns = Array.isArray(result?.callsigns)
            ? result.callsigns
            : Array.isArray(result)
              ? result
              : [];
          setSupercheckpartialCallsigns(callsigns);
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
          setDxccData(result?.dxcc ?? result ?? null);
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

  return {
    activeCompletionField,
    setActiveCompletionField,
    completionMatches,
    currentDxccLabel,
    currentDupeAlertText,
  };
}
