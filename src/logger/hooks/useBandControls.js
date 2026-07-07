import {
  bandByName,
  isSelectableMode,
  steppedFrequencyHz,
  tuningIncrementHzForMode,
} from '../mainWindowHelpers';
import { lastCqFrequencyForBand } from '../../domain/bandMap';

export function useBandControls({
  operatingMode,
  radio,
  radioMode,
  radioFrequencyHz,
  currentBand,
  bandOptions,
  bandMapSpotStore,
  currentCallsign,
  onStoreCqFrequency,
  onMarkFrequency,
  onStoreBandMapSpot,
  onActivateBandMapSpot,
  onSetRadioFrequency,
  onSetRadioMode,
  onClearRit,
  onIncrementRit,
  onDecrementRit,
}) {
  function storeCurrentCqFrequency() {
    if (!currentBand) return;
    onStoreCqFrequency?.(radioFrequencyHz, currentBand.name);
  }

  function jumpToLastCqFrequency() {
    const frequencyHz = lastCqFrequencyForBand(
      bandMapSpotStore,
      currentBand?.name,
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
      ...new Map(bandOptions.map((band) => [band.name, band])).values(),
    ].sort((left, right) => left.lowerHz - right.lowerHz);
    const currentIndex = sortedBands.findIndex(
      (band) => band.name === currentBand.name,
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

  function handleBandChange(event) {
    const selectedBand = bandByName(bandOptions, String(event.target.value));

    if (selectedBand) {
      onSetRadioFrequency?.(selectedBand.lowerHz);
      if (isSelectableMode(radioMode)) {
        onSetRadioMode?.(radioMode);
      }
    }
  }

  return {
    storeCurrentCqFrequency,
    jumpToLastCqFrequency,
    markCurrentFrequency,
    storeCurrentBandMapSpot,
    activateBandMapSpot,
    clearRitIfEnabled,
    tuneByIncrement,
    shiftBand,
    handleBandChange,
  };
}
