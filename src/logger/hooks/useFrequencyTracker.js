import { useEffect, useRef } from 'react';
import { callsignClearThresholdHz, loggerFrequencyChangeAction } from '../mainWindowHelpers';

export function useFrequencyTracker({
  operatingMode,
  radioFrequencyHz,
  radioMode,
  pendingBandMapTuneFrequencyRef,
  setOperatingMode,
  clearEntryFields,
}) {
  const previousRadioFrequencyHzRef = useRef(radioFrequencyHz);

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
  }, [
    operatingMode,
    radioFrequencyHz,
    radioMode,
    pendingBandMapTuneFrequencyRef,
    setOperatingMode,
    clearEntryFields,
  ]);
}
