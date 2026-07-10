import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  bandMapSpots,
  deleteBandMapSpot,
  saveBandMapSpot,
} from '../../lib/api.js';
import {
  addBandMapSpot,
  createBandMapSpotStore,
  removeBandMapSpot,
} from '../../domain/bandMap.js';
import {
  BAND_MAP_ENABLED_STORAGE_KEY,
  bandForFrequency,
} from '../../logger/mainWindowHelpers.js';

function normalizedBandMapSequence(value) {
  const sequence = Number(value);
  return Number.isInteger(sequence) && sequence > 0 ? sequence : 0;
}

export function isBandMapSequenceMessage(message) {
  return (
    message?.type === 'bandmap_spot' ||
    message?.type === 'bandmap_spot_deleted' ||
    message?.type === 'bandmap_sequence'
  );
}

export function applyBandMapSequenceMessage({ store, sequence, message }) {
  if (!isBandMapSequenceMessage(message)) {
    return { store, sequence, applied: false, needsRefresh: false };
  }

  const messageSequence = normalizedBandMapSequence(message?.sequence);
  if (!messageSequence || messageSequence <= sequence) {
    return { store, sequence, applied: false, needsRefresh: false };
  }
  if (messageSequence !== sequence + 1) {
    return {
      store,
      sequence,
      applied: false,
      needsRefresh: true,
      messageSequence,
    };
  }

  if (message.type === 'bandmap_spot') {
    return {
      store: addBandMapSpot(store, message.spot),
      sequence: messageSequence,
      applied: true,
      needsRefresh: false,
    };
  }
  if (message.type === 'bandmap_spot_deleted') {
    return {
      store: removeBandMapSpot(store, message.id),
      sequence: messageSequence,
      applied: true,
      needsRefresh: false,
    };
  }

  return {
    store,
    sequence: messageSequence,
    applied: true,
    needsRefresh: false,
  };
}

export function visibleBandMapSpotStoreForCurrentBand({
  store,
  settings,
  radioFrequencyHz,
}) {
  const baseStore = store ?? createBandMapSpotStore();
  const allowedBands = settings?.allowed_bands ?? [];
  const bandCatalog = settings?.band_catalog ?? [];
  const currentBand = bandForFrequency(Number(radioFrequencyHz), bandCatalog);

  if (!currentBand?.name) return createBandMapSpotStore();

  return createBandMapSpotStore(
    (baseStore.sortedSpots ?? []).filter((spot) => {
      const band = bandForFrequency(Number(spot?.frequency_hz), bandCatalog);
      if (!band || band.name !== currentBand.name) return false;
      return allowedBands.length === 0 || allowedBands.includes(band.name);
    }),
  );
}

export function useBandMap({
  settings,
  enabled,
  logId,
  radioId,
  radioFrequencyHz,
  sendRadioMessage,
  notifyOperationalError,
  onBeforeActivateSpot,
}) {
  const [bandMapSpotStore, setBandMapSpotStore] = useState(() =>
    createBandMapSpotStore(),
  );
  const [bandMapSelection, setBandMapSelection] = useState(null);
  const bandMapEnabledRef = useRef(false);
  const bandMapSelectionSequenceRef = useRef(0);
  const bandMapSyncRef = useRef({
    status: 'idle',
    sequence: 0,
    bufferedMessages: [],
    refreshInFlight: false,
    generation: 0,
  });

  const sendBandMapSubscription = useCallback(
    (nextEnabled) => {
      sendRadioMessage?.({
        type: 'set_bandmap_enabled',
        enabled: nextEnabled,
      });
    },
    [sendRadioMessage],
  );

  const resetBandMapSync = useCallback((status) => {
    const sync = bandMapSyncRef.current;
    sync.status = status;
    sync.sequence = 0;
    sync.bufferedMessages = [];
    sync.refreshInFlight = false;
    sync.generation += 1;
  }, []);

  const warnBandMapSequenceGap = useCallback((reason, expectedSequence, result) => {
    console.warn('[BandMap] WARNING: sequence gap detected; refreshing snapshot.', {
      reason,
      expectedSequence,
      receivedSequence: result?.messageSequence ?? null,
      currentSequence: expectedSequence - 1,
      messageType: result?.message?.type ?? null,
    });
  }, []);

  const refreshBandMapSnapshot = useCallback(
    async (reason) => {
      const sync = bandMapSyncRef.current;
      if (!bandMapEnabledRef.current || sync.refreshInFlight) return;

      sync.refreshInFlight = true;
      sync.status = 'loading_snapshot';
      const generation = sync.generation;

      try {
        const result = await bandMapSpots({ logId });
        if (!bandMapEnabledRef.current) return;
        if (bandMapSyncRef.current.generation !== generation) return;

        const spots = Array.isArray(result?.spots)
          ? result.spots
          : Array.isArray(result)
            ? result
            : [];
        let nextStore = createBandMapSpotStore(spots);
        let nextSequence = normalizedBandMapSequence(result?.sequence);
        const bufferedMessages = sync.bufferedMessages;
        sync.bufferedMessages = [];

        for (const message of bufferedMessages) {
          const applied = applyBandMapSequenceMessage({
            store: nextStore,
            sequence: nextSequence,
            message,
          });
          if (applied.needsRefresh) {
            warnBandMapSequenceGap(reason, nextSequence + 1, {
              ...applied,
              message,
            });
            sync.refreshInFlight = false;
            void refreshBandMapSnapshot('buffered_sequence_gap');
            return;
          }
          nextStore = applied.store;
          nextSequence = applied.sequence;
        }

        sync.sequence = nextSequence;
        sync.status = 'live';
        setBandMapSpotStore(nextStore);
      } catch (error) {
        if (bandMapSyncRef.current.generation !== generation) return;
        sync.status = 'awaiting_ready';
        notifyOperationalError(
          'loadBandMapSpots',
          'Unable to load band map spots.',
          error,
        );
      } finally {
        if (bandMapSyncRef.current.generation === generation) {
          sync.refreshInFlight = false;
        }
      }
    },
    [logId, notifyOperationalError, warnBandMapSequenceGap],
  );

  const handleActivateBandMapSpot = useCallback(
    (spot) => {
      const frequencyHz = Number(spot?.frequency_hz);
      if (!frequencyHz) return;
      onBeforeActivateSpot?.();
      sendRadioMessage?.({ type: 'set_frequency', frequency_hz: frequencyHz });
      const callsign = String(spot?.call_dx ?? '').trim();
      if (!callsign) return;
      bandMapSelectionSequenceRef.current += 1;
      setBandMapSelection({
        sequence: bandMapSelectionSequenceRef.current,
        spot,
      });
    },
    [onBeforeActivateSpot, sendRadioMessage],
  );

  const handleStoreCqFrequency = useCallback(
    async (frequencyHz) => {
      try {
        await saveBandMapSpot({
          spot_type: 'cq',
          frequency_hz: frequencyHz,
          radio_id: radioId,
        });
      } catch (error) {
        notifyOperationalError(
          'storeCqBandMapSpot',
          'Unable to store CQ mark.',
          error,
        );
      }
    },
    [notifyOperationalError, radioId],
  );

  const handleMarkFrequency = useCallback(
    async (frequencyHz) => {
      try {
        await saveBandMapSpot({
          spot_type: 'in_use',
          frequency_hz: frequencyHz,
        });
      } catch (error) {
        notifyOperationalError(
          'storeInUseBandMapSpot',
          'Unable to store in-use mark.',
          error,
        );
      }
    },
    [notifyOperationalError],
  );

  const handleStoreBandMapSpot = useCallback(
    async (payload) => {
      try {
        await saveBandMapSpot({
          ...payload,
          radio_id: payload.radio_id ?? radioId,
          log_id: payload.log_id ?? logId,
        });
      } catch (error) {
        notifyOperationalError(
          'storeBandMapSpot',
          'Unable to store band map spot.',
          error,
        );
      }
    },
    [logId, notifyOperationalError, radioId],
  );

  const handleDeleteBandMapSpot = useCallback(
    async (spot) => {
      try {
        await deleteBandMapSpot(spot?.id);
      } catch (error) {
        notifyOperationalError(
          'deleteBandMapSpot',
          'Unable to delete band map spot.',
          error,
        );
      }
    },
    [notifyOperationalError],
  );

  useEffect(() => {
    localStorage.setItem(BAND_MAP_ENABLED_STORAGE_KEY, enabled ? '1' : '0');
  }, [enabled]);

  useEffect(() => {
    bandMapEnabledRef.current = enabled;
    if (enabled) {
      setBandMapSpotStore(createBandMapSpotStore());
      resetBandMapSync('awaiting_ready');
    } else {
      resetBandMapSync('idle');
    }
    sendBandMapSubscription(enabled);
  }, [enabled, resetBandMapSync, sendBandMapSubscription]);

  const visibleBandMapSpotStore = useMemo(
    () =>
      visibleBandMapSpotStoreForCurrentBand({
        store: bandMapSpotStore,
        settings,
        radioFrequencyHz,
      }),
    [bandMapSpotStore, radioFrequencyHz, settings],
  );

  const handleSocketOpenReload = useCallback(async () => {
    if (!bandMapEnabledRef.current) return;
    setBandMapSpotStore(createBandMapSpotStore());
    resetBandMapSync('awaiting_ready');
    sendBandMapSubscription(true);
  }, [resetBandMapSync, sendBandMapSubscription]);

  const handleSocketMessage = useCallback(
    (message) => {
      if (message.type === 'bandmap_subscription_ready') {
        if (!bandMapEnabledRef.current) return;
        void refreshBandMapSnapshot('subscription_ready');
        return;
      }
      if (!isBandMapSequenceMessage(message)) return;

      const sync = bandMapSyncRef.current;
      if (sync.status !== 'live') {
        sync.bufferedMessages.push(message);
        return;
      }

      setBandMapSpotStore((currentStore) => {
        const result = applyBandMapSequenceMessage({
          store: currentStore,
          sequence: sync.sequence,
          message,
        });
        if (result.needsRefresh) {
          warnBandMapSequenceGap('live_sequence_gap', sync.sequence + 1, {
            ...result,
            message,
          });
          sync.status = 'loading_snapshot';
          sync.bufferedMessages = [];
          void refreshBandMapSnapshot('live_sequence_gap');
          return currentStore;
        }
        sync.sequence = result.sequence;
        return result.store;
      });
    },
    [refreshBandMapSnapshot, warnBandMapSequenceGap],
  );

  return {
    visibleBandMapSpotStore,
    bandMapSelection,
    handleActivateBandMapSpot,
    handleStoreCqFrequency,
    handleMarkFrequency,
    handleStoreBandMapSpot,
    handleDeleteBandMapSpot,
    handleSocketOpenReload,
    handleSocketMessage,
  };
}
