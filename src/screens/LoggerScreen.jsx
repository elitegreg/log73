import React, { useEffect, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { apiJson } from '../lib/api';
import BandMapWindow from '../logger/BandMapWindow';
import LogWindow from '../logger/LogWindow';
import MainWindow from '../logger/MainWindow';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { BAND_MAP_ENABLED_STORAGE_KEY } from '../logger/mainWindowHelpers';
import {
  contactAdif,
  contactIdentifier,
  contactMeta,
  metaValue,
  sortContacts,
} from './loggerScreenHelpers';
import { formatSocketDebugTimestamp } from './loggerScreen/backendSocketController';
import { useBackendSocket } from './loggerScreen/useBackendSocket';
import { useBandMap } from './loggerScreen/useBandMap';
import { useContactsOutbox } from './loggerScreen/useContactsOutbox';
import { useLoggerContext } from './loggerScreen/useLoggerContext';
import { useLoggerImage } from './loggerScreen/useLoggerImage';
import { useOperationalErrorReporter } from './loggerScreen/useOperationalErrorReporter';
import { useSerialAllocator } from './loggerScreen/useSerialAllocator';
import { getSessionId } from './loggerScreenHelpers';

function LoggerScreen() {
  const { logId, radioId } = useParams();
  const navigate = useNavigate();
  const numericLogId = Number(logId);
  const numericRadioId = Number(radioId);
  const [sessionId] = useState(getSessionId);
  const [bandMapEnabled, setBandMapEnabled] = useState(() => {
    return localStorage.getItem(BAND_MAP_ENABLED_STORAGE_KEY) === '1';
  });
  const loggerMainColumnRef = useRef(null);
  const [bandMapHeight, setBandMapHeight] = useState(null);
  const { notifyError, notifyOperationalError, notifyOfflineCachingDegraded } =
    useOperationalErrorReporter('LoggerScreen');

  const {
    settings,
    log,
    radio,
    messageLabels,
    operatorCallsign,
    isContextLoading,
  } = useLoggerContext(numericLogId, numericRadioId, {
    notifyOperationalError,
  });

  const socketOpenHandlerRef = useRef(null);
  const socketMessageHandlerRef = useRef(null);
  const remoteContactHandlerRef = useRef(null);
  const remoteContactDeletedHandlerRef = useRef(null);
  const refreshContactsHandlerRef = useRef(null);

  const {
    radioState,
    backendSocketStatus,
    catStatus,
    messageSentEvent,
    scoreSummary,
    isSocketDebugPanelEnabled,
    socketDebugEntries,
    sendRadioMessage,
  } = useBackendSocket({
    sessionId,
    numericLogId,
    numericRadioId,
    notifyOperationalError,
    onSocketOpenRef: socketOpenHandlerRef,
    onSocketMessageRef: socketMessageHandlerRef,
    onRemoteContactRef: remoteContactHandlerRef,
    onRemoteContactDeletedRef: remoteContactDeletedHandlerRef,
    onRefreshContactsRef: refreshContactsHandlerRef,
  });

  const {
    visibleBandMapSpotStore,
    bandMapSelection,
    handleActivateBandMapSpot,
    handleStoreCqFrequency,
    handleMarkFrequency,
    handleStoreBandMapSpot,
    handleSocketOpenReload,
    handleSocketMessage,
  } = useBandMap({
    settings,
    enabled: bandMapEnabled,
    sendRadioMessage,
    notifyOperationalError,
  });

  const {
    allContacts,
    setAllContacts,
    visibleContacts,
    handleDebouncedCallsignChange,
    contactsLoadState,
    hasMoreContacts,
    isLoadingMoreContacts,
    refreshContacts,
    loadMoreContacts,
    upsertRemoteContact,
    removeRemoteContact,
  } = useContactsOutbox({
    logId,
    numericLogId,
    sessionId,
    backendSocketStatus,
    notifyOperationalError,
    notifyOfflineCachingDegraded,
  });

  const { serialAllocationStatus, handleSerialContactLogged } =
    useSerialAllocator({
      settings,
      log,
      numericLogId,
      notifyOfflineCachingDegraded,
    });

  const loggerImageSrc = useLoggerImage();

  socketOpenHandlerRef.current = handleSocketOpenReload;
  socketMessageHandlerRef.current = handleSocketMessage;
  remoteContactHandlerRef.current = upsertRemoteContact;
  remoteContactDeletedHandlerRef.current = removeRemoteContact;
  refreshContactsHandlerRef.current = refreshContacts;

  useEffect(() => {
    const element = loggerMainColumnRef.current;
    if (!element || typeof ResizeObserver === 'undefined') {
      setBandMapHeight(null);
      return undefined;
    }

    const updateHeight = () => setBandMapHeight(element.offsetHeight || null);
    updateHeight();
    const observer = new ResizeObserver(updateHeight);
    observer.observe(element);
    return () => observer.disconnect();
  }, [
    bandMapEnabled,
    isContextLoading,
    contactsLoadState,
    visibleContacts.length,
  ]);

  async function handleRescore() {
    if (contactsLoadState !== 'idle') return;
    await refreshContacts({ mode: 'refresh', reset: true });
  }

  async function deleteContacts(contactsToDelete) {
    if (contactsToDelete.length === 0) return;

    const qsoLabel =
      contactsToDelete.length === 1
        ? '1 QSO'
        : `${contactsToDelete.length} QSOs`;
    if (!window.confirm(`Are you sure you want to delete ${qsoLabel}?`)) return;

    const committedContacts = contactsToDelete.filter(
      (contact) => metaValue(contact, 'id') !== undefined,
    );
    const localContactIdentifiers = contactsToDelete
      .filter((contact) => metaValue(contact, 'id') === undefined)
      .map(contactIdentifier)
      .filter(Boolean);
    const successfullyDeletedIds = [];
    const results = await Promise.allSettled(
      committedContacts.map(async (contact) => {
        const contactId = metaValue(contact, 'id');
        const result = await apiJson(`/contacts/${contactId}`, {
          method: 'DELETE',
        });
        if ((result?.deleted ?? result) === true) {
          successfullyDeletedIds.push(String(contactId));
        }
      }),
    );
    const failureCount = results.filter(
      (result) => result.status === 'rejected',
    ).length;
    const deleteFailures = results
      .map((result, index) => {
        if (result.status !== 'rejected') return null;
        return {
          id: metaValue(committedContacts[index], 'id') ?? null,
          error: errorMessage(result.reason, 'Unable to delete contact'),
        };
      })
      .filter(Boolean);
    const deletedIdentifiers = new Set([
      ...successfullyDeletedIds.map((id) => `id:${id}`),
      ...localContactIdentifiers,
    ]);

    setAllContacts((currentContacts) =>
      currentContacts.filter((contact) => {
        const identifier = contactIdentifier(contact);
        return !identifier || !deletedIdentifiers.has(identifier);
      }),
    );
    if (failureCount > 0) {
      const message = `Unable to delete ${failureCount === 1 ? '1 QSO' : `${failureCount} QSOs`}.`;
      notifyError(message, {
        dedupeKey: `LoggerScreen.deleteContacts:${failureCount}`,
      });
      reportClientErrorLater({
        source: 'LoggerScreen.deleteContacts',
        message,
        details: {
          logId: numericLogId,
          failures: deleteFailures,
        },
      });
    }
  }

  function updateContacts(contactsToUpdate, field, value) {
    const identifiers = new Set(
      contactsToUpdate.map(contactIdentifier).filter(Boolean),
    );
    if (identifiers.size === 0) return;

    setAllContacts((currentContacts) =>
      sortContacts(
        currentContacts.map((contact) => {
          const identifier = contactIdentifier(contact);
          if (!identifier || !identifiers.has(identifier)) return contact;
          return {
            meta: {
              ...contactMeta(contact),
              status:
                metaValue(contact, 'id') === undefined ? 'Pending' : 'Updating',
              error: undefined,
            },
            adif: {
              ...contactAdif(contact),
              [field]: value,
            },
          };
        }),
      ),
    );
  }

  function exitLogger() {
    navigate('/ui/open_log');
  }

  return (
    <div className="app-container">
      <div className="logger-workspace">
        {loggerImageSrc ? (
          <div className="logger-image-panel" aria-hidden="true">
            <img className="logger-side-image" src={loggerImageSrc} alt="" />
          </div>
        ) : null}
        <div className="logger-main-column" ref={loggerMainColumnRef}>
          <MainWindow
            settings={settings}
            log={log}
            radio={radio}
            isContextLoading={isContextLoading}
            contactsLoadState={contactsLoadState}
            contacts={visibleContacts}
            lastContact={allContacts[0] ?? null}
            stationCallsign={log?.station_callsign ?? ''}
            operatorCallsign={operatorCallsign}
            radioState={radioState}
            backendSocketStatus={backendSocketStatus}
            catStatus={catStatus}
            messageLabels={messageLabels}
            messageSentEvent={messageSentEvent}
            sessionId={sessionId}
            logId={numericLogId}
            bandMapEnabled={bandMapEnabled}
            bandMapSpotStore={visibleBandMapSpotStore}
            bandMapSelection={bandMapSelection}
            onSetBandMapEnabled={setBandMapEnabled}
            onActivateBandMapSpot={handleActivateBandMapSpot}
            onStoreCqFrequency={handleStoreCqFrequency}
            onMarkFrequency={handleMarkFrequency}
            onStoreBandMapSpot={handleStoreBandMapSpot}
            onSetRadioFrequency={(frequencyHz) =>
              sendRadioMessage({
                type: 'set_frequency',
                frequency_hz: frequencyHz,
              })
            }
            onSetRadioMode={(mode) =>
              sendRadioMessage({ type: 'set_mode', mode })
            }
            onClearRit={() => sendRadioMessage({ type: 'rit_clear' })}
            onIncrementRit={(hz) =>
              sendRadioMessage({ type: 'rit_increment', hz })
            }
            onDecrementRit={(hz) =>
              sendRadioMessage({ type: 'rit_decrement', hz })
            }
            onSendMessage={(payload) =>
              sendRadioMessage({ type: 'send_message', ...payload })
            }
            onSendCwText={(payload) =>
              sendRadioMessage({ type: 'send_cw_text', ...payload })
            }
            onSendDxClusterSpot={(payload) =>
              sendRadioMessage({ type: 'send_dxcluster_spot', ...payload })
            }
            onStopKeying={() => sendRadioMessage({ type: 'stop_keying' })}
            onSetCwWpm={(wpm) => sendRadioMessage({ type: 'set_wpm', wpm })}
            onDebouncedCallsignChange={handleDebouncedCallsignChange}
            onLogContact={(contact) => {
              setAllContacts((currentContacts) =>
                sortContacts([...currentContacts, contact]),
              );
            }}
            onRescore={handleRescore}
            isRescoreLoading={false}
            scoreSummary={scoreSummary}
            serialAllocation={serialAllocationStatus}
            onSerialContactLogged={handleSerialContactLogged}
            onExit={exitLogger}
          />
          <LogWindow
            settings={settings}
            contacts={visibleContacts}
            log={log}
            contactsLoadState={contactsLoadState}
            radioMode={radioState?.mode ?? 'CW'}
            onDeleteContacts={deleteContacts}
            onUpdateContacts={updateContacts}
            hasMoreContacts={hasMoreContacts}
            isLoadingMoreContacts={isLoadingMoreContacts}
            onLoadMoreContacts={loadMoreContacts}
          />
        </div>
        {bandMapEnabled ? (
          <BandMapWindow
            spotStore={visibleBandMapSpotStore}
            radioFrequencyHz={radioState?.frequency_hz}
            height={bandMapHeight}
            onSpotClick={handleActivateBandMapSpot}
          />
        ) : null}
      </div>
      {isSocketDebugPanelEnabled && (
        <div
          style={{
            width: '100%',
            maxWidth: '1600px',
            marginTop: '8px',
            border: '1px solid #808080',
            backgroundColor: '#f7f7f7',
            color: '#111',
            fontFamily: 'monospace',
            fontSize: '11px',
            lineHeight: '1.35',
          }}
        >
          <div
            style={{
              padding: '4px 6px',
              borderBottom: '1px solid #c0c0c0',
              backgroundColor: '#e8e8e8',
              fontWeight: 'bold',
            }}
          >
            Logger websocket debug
          </div>
          <div
            style={{
              maxHeight: '180px',
              overflowY: 'auto',
              padding: '4px 6px',
            }}
          >
            {socketDebugEntries.length === 0 ? (
              <div>Waiting for websocket events...</div>
            ) : (
              socketDebugEntries.map((entry) => (
                <div
                  key={entry.id}
                  style={{
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-word',
                    borderBottom: '1px dotted #d0d0d0',
                    padding: '1px 0',
                  }}
                >
                  {formatSocketDebugTimestamp(entry.timestamp)} {entry.event}
                  {entry.detailsText ? ` ${entry.detailsText}` : ''}
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export default LoggerScreen;
