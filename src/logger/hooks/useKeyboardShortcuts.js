import { useEffect } from 'react';
import {
  FUNCTION_KEY_PATTERN,
  isPageDownKey,
  isPageUpKey,
  modeIsCw,
  nextCwWpm,
} from '../mainWindowHelpers';
import {
  nextBandMapSpotAbove,
  nextBandMapSpotBelow,
} from '../../domain/bandMap';

export function useKeyboardShortcuts({
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
}) {
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
        !event.ctrlKey &&
        event.altKey &&
        !event.metaKey &&
        isPageUpKey(event)
      ) {
        event.preventDefault();
        shiftBand(1);
        return;
      }
      if (
        !event.ctrlKey &&
        event.altKey &&
        !event.metaKey &&
        isPageDownKey(event)
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
        isPageUpKey(event)
      ) {
        event.preventDefault();
        setCwWpm((current) => nextCwWpm(current, 1));
        return;
      }
      if (
        !event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        isPageDownKey(event)
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
}
