import { useCallback, useEffect, useRef, useState } from 'react';
import { apiJson } from '../../lib/api';
import { reportClientErrorLater } from '../../lib/errorReporting';
import {
  SERIAL_ALLOCATION_RETRY_DELAY_MS,
  appendSerialRange,
  getSerialInstanceId,
  loadSerialAllocation,
  reserveNextSerial,
  saveSerialAllocation,
  sentSerialField,
  serialBatchSize,
  serialRangesRemaining,
  serialRefillRemainingThreshold,
} from '../loggerScreenHelpers.js';
import {
  shouldRequestSerialRefill,
  unavailableSerialMessage,
} from './serialAllocatorState.js';

export function useSerialAllocator({
  settings,
  log,
  numericLogId,
  notifyOfflineCachingDegraded,
}) {
  const serialAllocatorRef = useRef(null);
  const [serialAllocationStatus, setSerialAllocationStatus] = useState({
    required: false,
    available: true,
    current: null,
    message: '',
  });

  useEffect(() => {
    const field = sentSerialField(settings);
    if (!field || !numericLogId) {
      serialAllocatorRef.current = null;
      setSerialAllocationStatus({
        required: false,
        available: true,
        current: null,
        message: '',
      });
      return;
    }

    let cancelled = false;
    const batchSize = serialBatchSize(log?.contest_params ?? {});
    const threshold = serialRefillRemainingThreshold(batchSize);
    const instanceId = getSerialInstanceId();
    const manager = {
      allocation: loadSerialAllocation(numericLogId, field.adif, instanceId),
      batchSize,
      current: null,
      errorReported: false,
      field,
      instanceId,
      message: '',
      requestInFlight: false,
      retryTimerId: undefined,
    };
    serialAllocatorRef.current = manager;

    function isActive() {
      return !cancelled && serialAllocatorRef.current === manager;
    }

    function remaining() {
      return serialRangesRemaining(manager.allocation);
    }

    function publish() {
      if (!isActive()) return;
      setSerialAllocationStatus({
        required: true,
        available: manager.current !== null && manager.current !== undefined,
        current: manager.current,
        message: manager.message,
        fieldAdif: field.adif,
      });
    }

    function persist() {
      if (
        !saveSerialAllocation(
          numericLogId,
          field.adif,
          manager.instanceId,
          manager.allocation,
        )
      ) {
        notifyOfflineCachingDegraded();
      }
    }

    function reserveLocalSerial() {
      if (manager.current !== null && manager.current !== undefined)
        return true;
      const reserved = reserveNextSerial(manager.allocation);
      if (!reserved) {
        manager.current = null;
        return false;
      }
      manager.allocation = reserved.allocation;
      manager.current = reserved.serial;
      persist();
      return true;
    }

    function clearRetryTimer() {
      if (manager.retryTimerId !== undefined) {
        window.clearTimeout(manager.retryTimerId);
        manager.retryTimerId = undefined;
      }
    }

    function scheduleRetry() {
      if (!isActive() || manager.retryTimerId !== undefined) return;
      manager.retryTimerId = window.setTimeout(() => {
        manager.retryTimerId = undefined;
        requestAllocation('retry');
      }, SERIAL_ALLOCATION_RETRY_DELAY_MS);
    }

    async function requestAllocation(reason) {
      if (!isActive() || manager.requestInFlight) return;
      clearRetryTimer();
      manager.requestInFlight = true;
      if (manager.current === null || manager.current === undefined) {
        manager.message = 'Requesting serial numbers...';
      }
      publish();

      try {
        const result = await apiJson(
          `/logs/${numericLogId}/serial-allocation`,
          {
            method: 'POST',
            body: JSON.stringify({
              field_adif: field.adif,
              count: manager.batchSize,
              reason,
            }),
          },
        );
        const allocation = result?.allocation ?? result ?? {};
        manager.allocation = appendSerialRange(
          manager.allocation,
          allocation.start,
          allocation.end,
        );
        persist();
        reserveLocalSerial();
        manager.errorReported = false;
        manager.message = '';
      } catch (error) {
        if (!isActive()) return;
        manager.message = unavailableSerialMessage(
          manager.current,
          remaining(),
        );
        if (!manager.errorReported) {
          manager.errorReported = true;
          reportClientErrorLater({
            source: 'LoggerScreen.serialAllocation',
            message: 'Unable to allocate sent serial numbers.',
            error,
            details: {
              logId: numericLogId,
              fieldAdif: field.adif,
              batchSize: manager.batchSize,
              reason,
            },
          });
        }
        scheduleRetry();
      } finally {
        if (isActive()) {
          manager.requestInFlight = false;
          publish();
        }
      }
    }

    function ensureSerials(reason) {
      reserveLocalSerial();
      publish();
      if (
        shouldRequestSerialRefill({
          current: manager.current,
          remaining: remaining(),
          threshold,
        })
      ) {
        requestAllocation(reason);
      }
    }

    manager.consumeLoggedSerial = () => {
      manager.current = null;
      manager.message = '';
      ensureSerials('after-log');
    };

    ensureSerials('startup');

    return () => {
      cancelled = true;
      clearRetryTimer();
      if (serialAllocatorRef.current === manager) {
        serialAllocatorRef.current = null;
      }
    };
  }, [
    settings,
    log?.contest_params,
    numericLogId,
    notifyOfflineCachingDegraded,
  ]);

  const handleSerialContactLogged = useCallback(() => {
    serialAllocatorRef.current?.consumeLoggedSerial?.();
  }, []);

  return {
    serialAllocationStatus,
    handleSerialContactLogged,
  };
}
