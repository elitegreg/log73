import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { dxclusterSpots, saveDxclusterSpot } from '../../lib/api';
import {
  addBandMapSpot,
  addCqBandMapSpot,
  addInUseBandMapSpot,
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
  sendRadioMessage,
  notifyOperationalError,
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
        type: 'set_dxcluster_enabled',
        enabled: nextEnabled,
      });
    },
    [sendRadioMessage],
  );

  const handleActivateBandMapSpot = useCallback(
    (spot) => {
      const frequencyHz = Number(spot?.frequency_hz);
      if (!frequencyHz) return;
      sendRadioMessage?.({ type: 'set_frequency', frequency_hz: frequencyHz });
      const callsign = String(spot?.call_dx ?? '').trim();
      if (!callsign) return;
      bandMapSelectionSequenceRef.current += 1;
      setBandMapSelection({
        sequence: bandMapSelectionSequenceRef.current,
        spot,
      });
    },
    [sendRadioMessage],
  );

  const handleStoreCqFrequency = useCallback((frequencyHz, bandMeters) => {
    setBandMapSpotStore((currentStore) =>
      addCqBandMapSpot(currentStore, frequencyHz, bandMeters),
    );
  }, []);

  const handleMarkFrequency = useCallback((frequencyHz) => {
    setBandMapSpotStore((currentStore) =>
      addInUseBandMapSpot(currentStore, frequencyHz),
    );
  }, []);

  const handleStoreBandMapSpot = useCallback(
    async (payload) => {
      try {
        const result = await saveDxclusterSpot(payload);
        const spot = result?.spot ?? result;
        if (spot) {
          setBandMapSpotStore((currentStore) =>
            addBandMapSpot(currentStore, spot),
          );
        }
      } catch (error) {
        notifyOperationalError(
          'storeBandMapSpot',
          'Unable to store band map spot.',
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
    dxclusterSpots()
      .then((result) => {
        if (isCancelled) return;
        const spots = Array.isArray(result?.spots)
          ? result.spots
          : Array.isArray(result)
            ? result
            : [];
        setBandMapSpotStore((currentStore) =>
          spots.reduce(addBandMapSpot, currentStore),
        );
      })
      .catch((error) =>
        notifyOperationalError(
          'loadBandMapSpots',
          'Unable to load band map spots.',
          error,
        ),
      );

    return () => {
      isCancelled = true;
    };
  }, [enabled, notifyOperationalError, sendBandMapSubscription]);

  const visibleBandMapSpotStore = useMemo(() => {
    const allowedBands = settings?.allowed_bands ?? [];
    if (allowedBands.length === 0) return bandMapSpotStore;

    return createBandMapSpotStore(
      (bandMapSpotStore?.sortedSpots ?? []).filter((spot) => {
        const band = bandForFrequency(Number(spot?.frequency_hz));
        return band ? allowedBands.includes(band.meters) : false;
      }),
    );
  }, [bandMapSpotStore, settings]);

  const handleSocketOpenReload = useCallback(async () => {
    if (!bandMapEnabledRef.current) return;
    setBandMapSpotStore(createBandMapSpotStore());
    try {
      const result = await dxclusterSpots();
      const spots = Array.isArray(result?.spots)
        ? result.spots
        : Array.isArray(result)
          ? result
          : [];
      setBandMapSpotStore((currentStore) =>
        spots.reduce(addBandMapSpot, currentStore),
      );
      sendBandMapSubscription(true);
    } catch (error) {
      notifyOperationalError(
        'reloadBandMapSpots',
        'Unable to reload band map spots.',
        error,
      );
    }
  }, [notifyOperationalError, sendBandMapSubscription]);

  const handleSocketMessage = useCallback((message) => {
    if (message.type === 'dxcluster_spot') {
      setBandMapSpotStore((currentStore) =>
        addBandMapSpot(currentStore, message.spot),
      );
    } else if (message.type === 'dxcluster_spot_deleted') {
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
    handleSocketOpenReload,
    handleSocketMessage,
  };
}
