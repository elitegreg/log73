import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { NotificationsContext } from './notificationsContext';
const DEFAULT_DURATION_MS = 6000;
const DEDUPE_WINDOW_MS = 3000;
const MAX_NOTIFICATIONS = 4;

function createNotificationId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function normalizedMessage(message) {
  return String(message ?? '').trim();
}

export function NotificationsProvider({ children }) {
  const [notifications, setNotifications] = useState([]);
  const dedupeRef = useRef(new Map());
  const timersRef = useRef(new Map());

  const dismiss = useCallback((id) => {
    const timer = timersRef.current.get(id);
    if (timer) {
      window.clearTimeout(timer);
      timersRef.current.delete(id);
    }
    setNotifications((current) =>
      current.filter((notification) => notification.id !== id),
    );
  }, []);

  const notify = useCallback(
    ({
      title = '',
      message,
      type = 'error',
      durationMs = DEFAULT_DURATION_MS,
      dedupeKey,
    }) => {
      const nextMessage = normalizedMessage(message);
      if (!nextMessage) return null;

      const dedupeValue = dedupeKey ?? `${type}|${title}|${nextMessage}`;
      const now = Date.now();
      const lastSentAt = dedupeRef.current.get(dedupeValue);
      if (typeof lastSentAt === 'number' && now - lastSentAt < DEDUPE_WINDOW_MS)
        return null;
      dedupeRef.current.set(dedupeValue, now);

      const id = createNotificationId();
      setNotifications((current) => {
        const next = [...current, { id, title, message: nextMessage, type }];
        if (next.length <= MAX_NOTIFICATIONS) return next;
        const overflow = next.length - MAX_NOTIFICATIONS;
        for (const removed of next.slice(0, overflow)) {
          const removedTimer = timersRef.current.get(removed.id);
          if (removedTimer) {
            window.clearTimeout(removedTimer);
            timersRef.current.delete(removed.id);
          }
        }
        return next.slice(overflow);
      });

      const timer = window.setTimeout(() => dismiss(id), durationMs);
      timersRef.current.set(id, timer);
      return id;
    },
    [dismiss],
  );

  useEffect(
    () => () => {
      for (const timer of timersRef.current.values()) {
        window.clearTimeout(timer);
      }
      timersRef.current.clear();
    },
    [],
  );

  const value = useMemo(
    () => ({
      notify,
      notifyError: (message, options = {}) =>
        notify({ ...options, type: 'error', message }),
      dismiss,
    }),
    [dismiss, notify],
  );

  return (
    <NotificationsContext.Provider value={value}>
      {children}
      <div className="notification-stack" role="status" aria-live="polite">
        {notifications.map((notification) => (
          <div
            key={notification.id}
            className={`notification notification-${notification.type}`}
          >
            <div className="notification-content">
              {notification.title ? (
                <div className="notification-title">{notification.title}</div>
              ) : null}
              <div className="notification-message">{notification.message}</div>
            </div>
            <button
              type="button"
              className="notification-dismiss"
              onClick={() => dismiss(notification.id)}
              aria-label="Dismiss notification"
            >
              ×
            </button>
          </div>
        ))}
      </div>
    </NotificationsContext.Provider>
  );
}
