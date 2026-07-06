import { useEffect, useState } from 'react';
import {
  loadLoggerImageUrl,
  loggerImageRefreshUrl,
} from '../../domain/loggerImageSettings';

const LOGGER_IMAGE_REFRESH_INTERVAL_MS = 60 * 60 * 1000;

export function useLoggerImage() {
  const [loggerImageUrl] = useState(loadLoggerImageUrl);
  const [loggerImageSrc, setLoggerImageSrc] = useState(null);

  useEffect(() => {
    if (!loggerImageUrl) {
      setLoggerImageSrc(null);
      return undefined;
    }

    let cancelled = false;
    let currentLoader = null;

    function tryLoadImage() {
      const refreshSrc = loggerImageRefreshUrl(loggerImageUrl, Date.now());
      if (!refreshSrc) return;
      const loader = new window.Image();
      currentLoader = loader;
      loader.onload = () => {
        if (cancelled || currentLoader !== loader) return;
        setLoggerImageSrc(refreshSrc);
      };
      loader.onerror = () => {
        if (cancelled || currentLoader !== loader) return;
      };
      loader.src = refreshSrc;
    }

    tryLoadImage();
    const intervalId = window.setInterval(
      tryLoadImage,
      LOGGER_IMAGE_REFRESH_INTERVAL_MS,
    );

    return () => {
      cancelled = true;
      if (currentLoader) {
        currentLoader.onload = null;
        currentLoader.onerror = null;
      }
      window.clearInterval(intervalId);
    };
  }, [loggerImageUrl]);

  return loggerImageSrc;
}
