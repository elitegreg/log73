import { useCallback } from 'react';
import { errorMessage, reportClientErrorLater } from '../../lib/errorReporting';
import { useNotifications } from '../../lib/notificationsContext';

export function useOperationalErrorReporter(sourcePrefix = '') {
  const { notifyError } = useNotifications();

  const notifyOperationalError = useCallback(
    (source, fallback, error, details = {}) => {
      const effectiveSource = sourcePrefix
        ? `${sourcePrefix}.${source}`
        : source;
      const message = errorMessage(error, fallback);
      notifyError(message, { dedupeKey: `${effectiveSource}:${message}` });
      reportClientErrorLater({
        source: effectiveSource,
        message,
        error,
        details,
      });
    },
    [notifyError, sourcePrefix],
  );

  const notifyOfflineCachingDegraded = useCallback(() => {
    notifyError(
      'Offline caching is degraded in this browser. Pending contacts and serial allocations may not be saved locally.',
      { dedupeKey: `${sourcePrefix || 'LoggerScreen'}.offlineCachingDegraded` },
    );
  }, [notifyError, sourcePrefix]);

  return {
    notifyError,
    notifyOperationalError,
    notifyOfflineCachingDegraded,
  };
}
