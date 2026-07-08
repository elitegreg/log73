import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  bandMapSpots,
  deleteBandMapSpot,
  saveBandMapSpot,
} from '../../lib/api';
import {
  addBandMapSpot,
  createBandMapSpotStore,
  removeBandMapSpot,
} from '../../domain/bandMap';
import {
  BAND_MAP_ENABLED_STORAGE_KEY,
  bandForFrequency,
} from '../../logger/mainWindowHelpers';

export function useBandMap({
  settings,
  enabled,
  logId,
  radioId,
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

  const sendBandMapSubscription = useCallback(
    (nextEnabled) => {
      sendRadioMessage?.({
        type: 'set_bandmap_enabled',
        enabled: nextEnabled,
      });
    },
    [sendRadioMessage],
  );

  const loadBandMapSpots = useCallback(async () => {
    const result = await bandMapSpots({ logId });
    const spots = Array.isArray(result?.spots)
      ? result.spots
      : Array.isArray(result)
        ? result
        : [];
    setBandMapSpotStore((currentStore) =>
      spots.reduce(addBandMapSpot, currentStore),
    );
  }, [logId]);

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
    }
    sendBandMapSubscription(enabled);

    if (!enabled) return undefined;

    let isCancelled = false;
    loadBandMapSpots().catch((error) => {
      if (isCancelled) return;
      notifyOperationalError(
        'loadBandMapSpots',
        'Unable to load band map spots.',
        error,
      );
    });

    return () => {
      isCancelled = true;
    };
  }, [
    enabled,
    loadBandMapSpots,
    notifyOperationalError,
    sendBandMapSubscription,
  ]);

  const visibleBandMapSpotStore = useMemo(() => {
    const allowedBands = settings?.allowed_bands ?? [];
    const bandCatalog = settings?.band_catalog ?? [];
    if (allowedBands.length === 0) return bandMapSpotStore;

    return createBandMapSpotStore(
      (bandMapSpotStore?.sortedSpots ?? []).filter((spot) => {
        const band = bandForFrequency(Number(spot?.frequency_hz), bandCatalog);
        return band ? allowedBands.includes(band.name) : false;
      }),
    );
  }, [bandMapSpotStore, settings]);

  const handleSocketOpenReload = useCallback(async () => {
    if (!bandMapEnabledRef.current) return;
    setBandMapSpotStore(createBandMapSpotStore());
    try {
      await loadBandMapSpots();
      sendBandMapSubscription(true);
    } catch (error) {
      notifyOperationalError(
        'reloadBandMapSpots',
        'Unable to reload band map spots.',
        error,
      );
    }
  }, [loadBandMapSpots, notifyOperationalError, sendBandMapSubscription]);

  const handleSocketMessage = useCallback((message) => {
    if (message.type === 'bandmap_spot') {
      setBandMapSpotStore((currentStore) =>
        addBandMapSpot(currentStore, message.spot),
      );
    } else if (message.type === 'bandmap_spot_deleted') {
      setBandMapSpotStore((currentStore) =>
        removeBandMapSpot(currentStore, message.id),
      );
    }
  }, []);

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
