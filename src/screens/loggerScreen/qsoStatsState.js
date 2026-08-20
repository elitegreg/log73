export const QSO_STATS_REFRESH_INTERVAL_MS = 15_000;

export function backendIsMarkedOffline(backendSocketStatus) {
  return backendSocketStatus !== 'connected';
}

export function operatorStats(stats, operatorCallsign) {
  const operator = String(operatorCallsign ?? '')
    .trim()
    .toUpperCase();
  if (!operator) return null;
  return stats?.by_operator?.[operator] ?? null;
}

export function formatQsoRate(stat) {
  if (stat?.rate_per_hour === null || stat?.rate_per_hour === undefined) {
    return '—';
  }
  const rate = Number(stat.rate_per_hour);
  return Number.isFinite(rate) ? String(Math.round(rate)) : '—';
}

export function reportQsoStatsFailure({
  error,
  backendSocketStatus,
  notifyOperationalError,
  logId,
  logger = console,
}) {
  logger.error('[LoggerScreen stats] Unable to load QSO rates.', error);
  if (backendIsMarkedOffline(backendSocketStatus)) return;
  notifyOperationalError('loadQsoStats', 'Unable to load QSO rates.', error, {
    logId,
  });
}
