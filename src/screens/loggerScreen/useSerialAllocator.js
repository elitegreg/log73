import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { apiJson } from '../../lib/api';
import { reportClientErrorLater } from '../../lib/errorReporting';
import {
  SERIAL_ALLOCATION_RETRY_DELAY_MS,
  getSerialInstanceId,
  loadUnusedSerial,
  saveUnusedSerial,
  sentSerialField,
} from '../loggerScreenHelpers.js';
import {
  currentSerialForBand,
  mergeSerialStates,
  serialStateAfterContact,
  serialStateFromBackend,
  unavailableSerialMessage,
} from './serialAllocatorState.js';

export function useSerialAllocator({
  settings,
  numericLogId,
  currentBandName,
  contacts = [],
  notifyOfflineCachingDegraded,
}) {
  const serialAllocatorRef = useRef(null);
  const [publishedState, setPublishedState] = useState(null);
  const [message, setMessage] = useState('');

  useEffect(() => {
    const field = sentSerialField(settings);
    if (!field || !numericLogId) {
      serialAllocatorRef.current = null;
      setPublishedState(null);
      setMessage('');
      return;
    }

    let cancelled = false;
    const instanceId = getSerialInstanceId();
    const manager = {
      errorReported: false,
      field,
      initialized: false,
      instanceId,
      pendingContacts: [],
      requestInFlight: false,
      retryTimerId: undefined,
      state: null,
    };
    serialAllocatorRef.current = manager;

    function isActive() {
      return !cancelled && serialAllocatorRef.current === manager;
    }

    function publish(nextMessage = '') {
      if (!isActive()) return;
      setPublishedState(manager.state ? { ...manager.state } : null);
      setMessage(nextMessage);
    }

    function persistUnused(serial) {
      if (
        !saveUnusedSerial(numericLogId, field.adif, manager.instanceId, serial)
      ) {
        notifyOfflineCachingDegraded();
      }
    }

    function clearRetryTimer() {
      if (manager.retryTimerId !== undefined) {
        window.clearTimeout(manager.retryTimerId);
        manager.retryTimerId = undefined;
      }
    }

    function scheduleRetry(action) {
      if (!isActive() || manager.retryTimerId !== undefined) return;
      manager.retryTimerId = window.setTimeout(() => {
        manager.retryTimerId = undefined;
        action();
      }, SERIAL_ALLOCATION_RETRY_DELAY_MS);
    }

    function reportAllocationError(error, operation) {
      if (manager.errorReported) return;
      manager.errorReported = true;
      reportClientErrorLater({
        source: 'LoggerScreen.serialAllocation',
        message: 'Unable to determine the next sent serial number.',
        error,
        details: {
          logId: numericLogId,
          fieldAdif: field.adif,
          operation,
        },
      });
    }

    async function requestReservation() {
      if (!isActive() || manager.requestInFlight) return;
      clearRetryTimer();
      manager.requestInFlight = true;
      publish('Requesting the next serial number...');

      try {
        const result = await apiJson(
          `/logs/${numericLogId}/serial-allocation`,
          {
            method: 'POST',
            body: JSON.stringify({ field_adif: field.adif }),
          },
        );
        const allocation = result?.allocation ?? result ?? {};
        const serial = Number.parseInt(
          String(allocation.serial ?? allocation.start),
          10,
        );
        if (!Number.isFinite(serial) || serial <= 0) {
          throw new Error('backend returned an invalid serial reservation');
        }
        manager.state = {
          ...manager.state,
          next: serial,
        };
        persistUnused(serial);
        manager.errorReported = false;
        publish();
      } catch (error) {
        if (!isActive()) return;
        reportAllocationError(error, 'reserve');
        publish(unavailableSerialMessage('global'));
        scheduleRetry(requestReservation);
      } finally {
        manager.requestInFlight = false;
      }
    }

    function observeContact(contact) {
      if (!manager.initialized) {
        manager.pendingContacts.push(contact);
        return;
      }

      const nextState = serialStateAfterContact(manager.state, contact);
      if (nextState === manager.state) return;

      if (manager.state.reservationRequired) {
        persistUnused(null);
        manager.state = { ...manager.state, next: null };
        publish('Requesting the next serial number...');
        requestReservation();
        return;
      }

      manager.state = nextState;
      publish();
    }
    manager.observeContact = observeContact;

    async function initialize() {
      if (!isActive() || manager.requestInFlight) return;
      clearRetryTimer();
      manager.requestInFlight = true;
      publish('Loading the next serial number...');

      try {
        const query = new URLSearchParams({ field_adif: field.adif });
        const result = await apiJson(
          `/logs/${numericLogId}/serial-allocation?${query.toString()}`,
        );
        const previousState = manager.state;
        manager.state = mergeSerialStates(
          serialStateFromBackend(result?.state ?? result),
          previousState,
        );
        manager.initialized = true;
        manager.errorReported = false;

        if (manager.state.reservationRequired) {
          manager.state.next = loadUnusedSerial(
            numericLogId,
            field.adif,
            manager.instanceId,
          );
        } else {
          persistUnused(null);
        }

        const pendingContacts = manager.pendingContacts;
        manager.pendingContacts = [];
        for (const contact of pendingContacts) {
          observeContact(contact);
        }

        if (
          manager.state.reservationRequired &&
          currentSerialForBand(manager.state, null) === null
        ) {
          publish('Requesting the next serial number...');
        } else {
          publish();
        }
      } catch (error) {
        if (!isActive()) return;
        reportAllocationError(error, 'initialize');
        publish(unavailableSerialMessage('global'));
        scheduleRetry(initialize);
      } finally {
        manager.requestInFlight = false;
        if (
          isActive() &&
          manager.initialized &&
          manager.state.reservationRequired &&
          currentSerialForBand(manager.state, null) === null
        ) {
          requestReservation();
        }
      }
    }
    manager.refresh = initialize;

    initialize();

    return () => {
      cancelled = true;
      clearRetryTimer();
      if (serialAllocatorRef.current === manager) {
        serialAllocatorRef.current = null;
      }
    };
  }, [settings, numericLogId, notifyOfflineCachingDegraded]);

  const handleSerialContactLogged = useCallback((contact) => {
    serialAllocatorRef.current?.observeContact?.(contact);
  }, []);

  useEffect(() => {
    for (const contact of contacts) {
      handleSerialContactLogged(contact);
    }
  }, [contacts, handleSerialContactLogged]);

  const refreshSerialState = useCallback(() => {
    serialAllocatorRef.current?.refresh?.();
  }, []);

  const serialAllocationStatus = useMemo(() => {
    const field = sentSerialField(settings);
    if (!field) {
      return {
        required: false,
        available: true,
        current: null,
        message: '',
      };
    }

    const current = currentSerialForBand(publishedState, currentBandName);
    const available = current !== null;
    return {
      required: true,
      available,
      current,
      message:
        message ||
        (available
          ? ''
          : unavailableSerialMessage(publishedState?.scope, currentBandName)),
      fieldAdif: field.adif,
      scope: publishedState?.scope ?? field.serial_scope ?? 'global',
    };
  }, [currentBandName, message, publishedState, settings]);

  return {
    serialAllocationStatus,
    handleSerialContactLogged,
    refreshSerialState,
  };
}
