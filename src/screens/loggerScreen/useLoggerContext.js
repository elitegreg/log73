import { useCallback, useEffect, useRef, useState } from 'react';
import { apiJson } from '../../lib/api';

function normalizeBand(band) {
  return {
    iaruRegion: Number(band?.iaru_region ?? 0),
    name: String(band?.name ?? ''),
    lowerHz: Number(band?.lower_hz ?? 0),
    upperHz: Number(band?.upper_hz ?? 0),
    defaultSsbMode: String(band?.default_ssb_mode ?? ''),
    sortOrder: Number(band?.sort_order ?? 0),
  };
}

let promptedOperatorCallsign;

export function useLoggerContext(logId, radioId, { notifyOperationalError }) {
  const [settings, setSettings] = useState(null);
  const [log, setLog] = useState(null);
  const [radio, setRadio] = useState(null);
  const [messageLabels, setMessageLabels] = useState(null);
  const [operatorCallsign, setOperatorCallsign] = useState('');
  const [isOperatorPromptOpen, setIsOperatorPromptOpen] = useState(false);
  const [isContextLoading, setIsContextLoading] = useState(true);
  const operatorCallsignRef = useRef('');

  const acceptOperatorCallsign = useCallback((callsign) => {
    promptedOperatorCallsign = String(callsign ?? '')
      .trim()
      .toUpperCase();
    operatorCallsignRef.current = promptedOperatorCallsign;
    setOperatorCallsign(promptedOperatorCallsign);
    setIsOperatorPromptOpen(false);
  }, []);

  useEffect(() => {
    let isCancelled = false;

    async function loadContext() {
      setIsContextLoading(true);
      const [
        logResult,
        radioResult,
        messageLabelsResult,
        bandCatalog,
        modeCatalog,
      ] = await Promise.all([
        apiJson(`/logs/${logId}`),
        apiJson(`/radios/${radioId}`),
        apiJson(`/radios/${radioId}/message-labels`),
        apiJson('/bands'),
        apiJson('/modes'),
      ]);
      const loadedLog = logResult?.log ?? logResult;
      const loadedRadio = radioResult?.radio ?? radioResult;
      const loadedMessageLabels =
        messageLabelsResult?.labels ?? messageLabelsResult;
      const contestSettings = await apiJson(
        `/contest-settings?contest_id=${encodeURIComponent(loadedLog.contest_id)}`,
      );
      if (isCancelled) return;
      setSettings({
        ...contestSettings,
        band_catalog: (bandCatalog ?? []).map(normalizeBand),
        mode_catalog: (modeCatalog ?? []).map((mode) =>
          String(mode).trim().toUpperCase(),
        ),
      });
      setLog(loadedLog);
      setRadio(loadedRadio);
      setMessageLabels(loadedMessageLabels);
      if (!operatorCallsignRef.current) {
        const defaultCallsign =
          promptedOperatorCallsign ?? loadedLog.station_callsign;
        operatorCallsignRef.current = defaultCallsign;
        setOperatorCallsign(defaultCallsign);
        setIsOperatorPromptOpen(true);
      }
    }

    const loadContextPromise = loadContext();
    loadContextPromise.catch((error) =>
      notifyOperationalError(
        'loadContext',
        'Unable to load logger context.',
        error,
        { logId, radioId },
      ),
    );
    loadContextPromise.finally(() => {
      if (!isCancelled) setIsContextLoading(false);
    });

    return () => {
      isCancelled = true;
    };
  }, [logId, radioId, notifyOperationalError]);

  useEffect(() => {
    function handleKeyDown(event) {
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key.toLowerCase() === 'o'
      ) {
        event.preventDefault();
        if (log) setIsOperatorPromptOpen(true);
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [log]);

  return {
    settings,
    log,
    radio,
    messageLabels,
    operatorCallsign,
    isOperatorPromptOpen,
    acceptOperatorCallsign,
    isContextLoading,
  };
}
