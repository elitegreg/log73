import { useCallback, useEffect, useRef, useState } from 'react';
import { apiJson } from '../../lib/api.js';
import {
  QSO_STATS_REFRESH_INTERVAL_MS,
  operatorStats,
  reportQsoStatsFailure,
} from './qsoStatsState.js';

export function useQsoStats({
  numericLogId,
  operatorCallsign,
  backendSocketStatus,
  notifyOperationalError,
}) {
  const [stats, setStats] = useState(null);
  const requestSequenceRef = useRef(0);

  const refreshStats = useCallback(async () => {
    if (!Number.isFinite(numericLogId)) return false;
    const requestSequence = requestSequenceRef.current + 1;
    requestSequenceRef.current = requestSequence;

    try {
      const nextStats = await apiJson(`/logs/${numericLogId}/stats`);
      if (requestSequence !== requestSequenceRef.current) return false;
      setStats(nextStats);
      return true;
    } catch (error) {
      if (requestSequence !== requestSequenceRef.current) return false;
      setStats(null);
      reportQsoStatsFailure({
        error,
        backendSocketStatus,
        notifyOperationalError,
        logId: numericLogId,
      });
      return false;
    }
  }, [backendSocketStatus, notifyOperationalError, numericLogId]);

  useEffect(() => {
    setStats(null);
    void refreshStats();
    const intervalId = window.setInterval(() => {
      if (!document.hidden) void refreshStats();
    }, QSO_STATS_REFRESH_INTERVAL_MS);
    const handleVisibilityChange = () => {
      if (!document.hidden) void refreshStats();
    };
    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => {
      window.clearInterval(intervalId);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      requestSequenceRef.current += 1;
    };
  }, [refreshStats]);

  return {
    overallStats: stats?.overall ?? null,
    currentOperatorStats: operatorStats(stats, operatorCallsign),
    refreshStats,
  };
}
