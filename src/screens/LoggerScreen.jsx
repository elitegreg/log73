import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { apiJson, websocketUrl } from '../lib/api';
import { errorMessage, reportClientErrorLater } from '../lib/errorReporting';
import { useNotifications } from '../lib/notificationsContext';
import LogWindow from '../logger/LogWindow';
import MainWindow from '../logger/MainWindow';
import {
  BACKEND_WS_INITIAL_RECONNECT_DELAY_MS,
  BACKEND_WS_MAX_RECONNECT_DELAY_MS,
  CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS,
  CONTACTS_LOAD_MAX_RETRY_DELAY_MS,
  CONTACTS_PAGE_SIZE,
  CONTACT_COMMIT_RETRY_DELAY_MS,
  DEFAULT_RADIO_STATE,
  EMPTY_SCORE_SUMMARY,
  getSessionId,
  loadLocalContacts,
  saveLocalContacts,
  committedBackendContact,
  mergeContact,
  sortContacts,
  markContactFailed,
  contactIdentifier,
} from './loggerScreenHelpers';

let promptedOperatorCallsign;

function promptForOperatorCallsign(defaultCallsign) {
  const enteredCallsign = window.prompt(
    'Operator Callsign',
    promptedOperatorCallsign ?? defaultCallsign,
  );
  if (enteredCallsign === null)
    return promptedOperatorCallsign ?? defaultCallsign;
  promptedOperatorCallsign = enteredCallsign.toUpperCase();
  return promptedOperatorCallsign;
}

function LoggerScreen() {
  const { logId, radioId } = useParams();
  const navigate = useNavigate();
  const { notifyError } = useNotifications();
  const numericLogId = Number(logId);
  const numericRadioId = Number(radioId);
  const [settings, setSettings] = useState(null);
  const [log, setLog] = useState(null);
  const [radio, setRadio] = useState(null);
  const [cwLabels, setCwLabels] = useState(null);
  const [cwSentEvent, setCwSentEvent] = useState(null);
  const [contacts, setContacts] = useState(() => loadLocalContacts(logId));
  const [operatorCallsign, setOperatorCallsign] = useState('');
  const [sessionId] = useState(getSessionId);
  const [radioState, setRadioState] = useState(DEFAULT_RADIO_STATE);
  const [backendSocketStatus, setBackendSocketStatus] =
    useState('disconnected');
  const [scoreSummary, setScoreSummary] = useState(EMPTY_SCORE_SUMMARY);
  const [isContextLoading, setIsContextLoading] = useState(true);
  const [contactsLoadState, setContactsLoadState] = useState('initial-loading');
  const [isRescoreLoading, setIsRescoreLoading] = useState(false);
  const backendSocketRef = useRef(null);
  const committingContactIdsRef = useRef(new Set());
  const refreshContactsRef = useRef(() => {});
  const contactsLoadErrorNotifiedRef = useRef(false);
  const commitContactErrorNotifiedRef = useRef(false);

  const notifyOperationalError = useCallback(
    (source, fallback, error, details = {}) => {
      const message = errorMessage(error, fallback);
      notifyError(message, { dedupeKey: `${source}:${message}` });
      reportClientErrorLater({
        source,
        message,
        error,
        details,
      });
    },
    [notifyError],
  );

  useEffect(() => {
    saveLocalContacts(logId, contacts);
  }, [contacts, logId]);

  useEffect(() => {
    setScoreSummary(EMPTY_SCORE_SUMMARY);
  }, [numericLogId]);

  useEffect(() => {
    let isCancelled = false;

    async function loadContext() {
      setIsContextLoading(true);
      const [logResult, radioResult, cwLabelsResult] = await Promise.all([
        apiJson(`/logs/${numericLogId}`),
        apiJson(`/radios/${numericRadioId}`),
        apiJson(`/radios/${numericRadioId}/cw-labels`),
      ]);
      if (!logResult.ok) throw new Error(logResult.error ?? 'Log not found');
      if (!radioResult.ok)
        throw new Error(radioResult.error ?? 'Radio not found');
      const contestSettings = await apiJson(
        `/contest-settings?contest_id=${encodeURIComponent(logResult.log.contest_id)}`,
      );
      if (isCancelled) return;
      setSettings(contestSettings);
      setLog(logResult.log);
      setRadio(radioResult.radio);
      if (cwLabelsResult.ok) setCwLabels(cwLabelsResult.labels);
      setOperatorCallsign(
        (current) =>
          current || promptForOperatorCallsign(logResult.log.station_callsign),
      );
    }
    const loadContextPromise = loadContext();
    loadContextPromise.catch((error) =>
      notifyOperationalError(
        'LoggerScreen.loadContext',
        'Unable to load logger context.',
        error,
        { logId: numericLogId, radioId: numericRadioId },
      ),
    );
    loadContextPromise.finally(() => {
      if (!isCancelled) setIsContextLoading(false);
    });
    return () => {
      isCancelled = true;
    };
  }, [numericLogId, numericRadioId, notifyOperationalError]);

  useEffect(() => {
    function handleKeyDown(event) {
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        event.key.toLowerCase() === 'o'
      ) {
        event.preventDefault();
        setOperatorCallsign(
          promptForOperatorCallsign(log?.station_callsign ?? ''),
        );
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [log]);

  useEffect(() => {
    let shouldReconnect = true;
    let reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
    let reconnectTimerId;

    function scheduleReconnect() {
      if (!shouldReconnect || reconnectTimerId !== undefined) return;
      reconnectTimerId = window.setTimeout(() => {
        reconnectTimerId = undefined;
        connectBackendSocket();
      }, reconnectDelayMs);
      reconnectDelayMs = Math.min(
        reconnectDelayMs * 2,
        BACKEND_WS_MAX_RECONNECT_DELAY_MS,
      );
    }

    function connectBackendSocket() {
      if (!shouldReconnect) return;
      const url = websocketUrl({
        session_id: sessionId,
        log_id: numericLogId,
        radio_id: numericRadioId,
      });
      setBackendSocketStatus('connecting');
      const socket = new WebSocket(url);
      backendSocketRef.current = socket;
      socket.addEventListener('open', () => {
        if (backendSocketRef.current !== socket) return;
        reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
        setBackendSocketStatus('connected');
        refreshContactsRef.current();
      });
      socket.addEventListener('message', (event) => {
        if (backendSocketRef.current !== socket) return;
        try {
          const message = JSON.parse(event.data);
          if (message.type === 'radio_state') {
            setRadioState({
              frequency_hz: message.frequency_hz,
              mode: message.mode,
            });
          } else if (message.type === 'cw_sent') {
            setCwSentEvent({
              requestId: message.request_id,
              sequence: Date.now(),
            });
          } else if (
            message.type === 'log_entry' &&
            message.contact?._session_id !== sessionId &&
            Number(message.contact?._log_id) === numericLogId
          ) {
            setContacts((currentContacts) =>
              mergeContact(currentContacts, message.contact),
            );
          } else if (
            message.type === 'contact_deleted' &&
            Number(message.log_id) === numericLogId
          ) {
            setContacts((currentContacts) =>
              currentContacts.filter(
                (contact) => String(contact._id) !== String(message.id),
              ),
            );
          } else if (
            message.type === 'score_update' &&
            Number(message.log_id) === numericLogId
          ) {
            setScoreSummary({
              qsoCount: Number(message.qso_count ?? 0),
              multipliers: Number(message.multipliers ?? 0),
              bonusPoints: Number(message.bonus_points ?? 0),
              score: Number(message.total_score ?? 0),
            });
          }
        } catch (error) {
          reportClientErrorLater({
            source: 'LoggerScreen.websocketMessage',
            message: 'Unable to process backend websocket message.',
            error,
            details: { logId: numericLogId, radioId: numericRadioId },
          });
        }
      });
      socket.addEventListener('close', () => {
        if (backendSocketRef.current === socket) {
          backendSocketRef.current = null;
          setBackendSocketStatus('disconnected');
          scheduleReconnect();
        }
      });
      socket.addEventListener('error', () => {
        if (backendSocketRef.current !== socket) return;
        setBackendSocketStatus('disconnected');
        socket.close();
      });
    }

    connectBackendSocket();
    return () => {
      shouldReconnect = false;
      if (reconnectTimerId !== undefined) window.clearTimeout(reconnectTimerId);
      const socket = backendSocketRef.current;
      backendSocketRef.current = null;
      socket?.close();
    };
  }, [sessionId, numericLogId, numericRadioId]);

  useEffect(() => {
    let shouldRetryContactsLoad = true;
    let contactsLoadRetryDelayMs = CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS;
    let contactsLoadRetryTimerId;
    let contactsLoadInFlightPromise = null;

    function scheduleContactsLoadRetry() {
      if (!shouldRetryContactsLoad) return false;
      if (contactsLoadRetryTimerId !== undefined) return true;
      contactsLoadRetryTimerId = window.setTimeout(() => {
        contactsLoadRetryTimerId = undefined;
        loadContacts({ mode: 'retry' });
      }, contactsLoadRetryDelayMs);
      contactsLoadRetryDelayMs = Math.min(
        contactsLoadRetryDelayMs * 2,
        CONTACTS_LOAD_MAX_RETRY_DELAY_MS,
      );
      return true;
    }

    function loadContacts({ mode = 'refresh' } = {}) {
      if (contactsLoadInFlightPromise) return contactsLoadInFlightPromise;
      setContactsLoadState((currentState) => {
        if (mode === 'retry') return 'retrying';
        if (mode === 'initial') return 'initial-loading';
        if (currentState === 'initial-loading') return 'initial-loading';
        return 'refreshing';
      });

      contactsLoadInFlightPromise = (async () => {
        try {
          const backendContacts = [];
          let offset = 0;

          while (true) {
            const page = await apiJson(
              `/logs/${numericLogId}/contacts?limit=${CONTACTS_PAGE_SIZE}&offset=${offset}`,
            );
            const committedPage = page.map(committedBackendContact);
            backendContacts.push(...committedPage);
            if (committedPage.length < CONTACTS_PAGE_SIZE) {
              break;
            }
            offset += CONTACTS_PAGE_SIZE;
          }

          const localUncommittedContacts = loadLocalContacts(logId).filter(
            (contact) => contact._status !== 'Committed',
          );
          setContacts(
            sortContacts([...backendContacts, ...localUncommittedContacts]),
          );
          shouldRetryContactsLoad = false;
          contactsLoadErrorNotifiedRef.current = false;
          setContactsLoadState('idle');
          return true;
        } catch (error) {
          if (!contactsLoadErrorNotifiedRef.current) {
            contactsLoadErrorNotifiedRef.current = true;
            notifyOperationalError(
              'LoggerScreen.loadContacts',
              'Unable to load backend contacts. Using local contacts and retrying.',
              error,
              { logId: numericLogId },
            );
          }
          const retryScheduled = scheduleContactsLoadRetry();
          setContactsLoadState(retryScheduled ? 'retrying' : 'idle');
          return false;
        } finally {
          contactsLoadInFlightPromise = null;
        }
      })();

      return contactsLoadInFlightPromise;
    }

    refreshContactsRef.current = loadContacts;
    setContactsLoadState('initial-loading');
    loadContacts({ mode: 'initial' });
    return () => {
      refreshContactsRef.current = () => {};
      shouldRetryContactsLoad = false;
      if (contactsLoadRetryTimerId !== undefined)
        window.clearTimeout(contactsLoadRetryTimerId);
    };
  }, [numericLogId, logId, notifyOperationalError]);

  useEffect(() => {
    const pendingContact = contacts.find((contact) => {
      if (contact._status === 'Pending')
        return (
          contact._client_id &&
          !committingContactIdsRef.current.has(contact._client_id)
        );
      if (contact._status === 'Updating') {
        const updateKey = contact._id ?? contact._client_id;
        return updateKey && !committingContactIdsRef.current.has(updateKey);
      }
      return false;
    });
    if (!pendingContact) return;

    const commitKey =
      pendingContact._status === 'Pending'
        ? pendingContact._client_id
        : (pendingContact._id ?? pendingContact._client_id);
    committingContactIdsRef.current.add(commitKey);

    async function commitContact(contact) {
      try {
        const responseBody = await apiJson(`/logs/${numericLogId}/contacts`, {
          method: 'POST',
          body: JSON.stringify({ ...contact, _log_id: numericLogId }),
        });
        if (!responseBody.ok) {
          notifyOperationalError(
            'LoggerScreen.commitContactRejected',
            responseBody.error ?? 'Contact upload failed.',
            responseBody.error,
            {
              logId: numericLogId,
              contactId: contact._id ?? contact._client_id ?? null,
            },
          );
          setContacts((currentContacts) =>
            markContactFailed(
              currentContacts,
              contact,
              responseBody.error ?? 'Contact upload failed.',
            ),
          );
          return;
        }
        if (responseBody.contact) {
          commitContactErrorNotifiedRef.current = false;
          setContacts((currentContacts) =>
            mergeContact(currentContacts, {
              ...responseBody.contact,
              _client_id: contact._client_id,
            }),
          );
        } else {
          notifyOperationalError(
            'LoggerScreen.commitContactMissing',
            'Contact upload failed: server response did not include a committed contact.',
            null,
            {
              logId: numericLogId,
              contactId: contact._id ?? contact._client_id ?? null,
            },
          );
          setContacts((currentContacts) =>
            markContactFailed(
              currentContacts,
              contact,
              'Contact upload failed: server response did not include a committed contact.',
            ),
          );
        }
      } catch (error) {
        if (!commitContactErrorNotifiedRef.current) {
          commitContactErrorNotifiedRef.current = true;
          notifyOperationalError(
            'LoggerScreen.commitContactRetry',
            'Unable to commit contact. Retrying.',
            error,
            {
              logId: numericLogId,
              contactId: contact._id ?? contact._client_id ?? null,
            },
          );
        }
        window.setTimeout(
          () => setContacts((currentContacts) => sortContacts(currentContacts)),
          CONTACT_COMMIT_RETRY_DELAY_MS,
        );
      } finally {
        committingContactIdsRef.current.delete(commitKey);
      }
    }

    commitContact(pendingContact);
  }, [contacts, numericLogId, notifyOperationalError]);

  function sendRadioMessage(message) {
    const socket = backendSocketRef.current;
    if (socket?.readyState === WebSocket.OPEN)
      socket.send(JSON.stringify(message));
  }

  async function handleRescore() {
    if (isRescoreLoading || contactsLoadState !== 'idle') return;
    setIsRescoreLoading(true);
    try {
      await refreshContactsRef.current({ mode: 'refresh' });
    } finally {
      setIsRescoreLoading(false);
    }
  }

  async function deleteContacts(contactsToDelete) {
    if (contactsToDelete.length === 0) return;

    const qsoLabel =
      contactsToDelete.length === 1
        ? '1 QSO'
        : `${contactsToDelete.length} QSOs`;
    if (!window.confirm(`Are you sure you want to delete ${qsoLabel}?`)) return;

    const committedContacts = contactsToDelete.filter(
      (contact) => contact._id !== undefined,
    );
    const localContactIdentifiers = contactsToDelete
      .filter((contact) => contact._id === undefined)
      .map(contactIdentifier)
      .filter(Boolean);
    const successfullyDeletedIds = [];
    const results = await Promise.allSettled(
      committedContacts.map(async (contact) => {
        const result = await apiJson(`/contacts/${contact._id}`, {
          method: 'DELETE',
        });
        if (!result.ok)
          throw new Error(result.error ?? 'Unable to delete contact');
        if (result.deleted) successfullyDeletedIds.push(String(contact._id));
      }),
    );
    const failureCount = results.filter(
      (result) => result.status === 'rejected',
    ).length;
    const deleteFailures = results
      .map((result, index) => {
        if (result.status !== 'rejected') return null;
        return {
          id: committedContacts[index]?._id ?? null,
          error: errorMessage(result.reason, 'Unable to delete contact'),
        };
      })
      .filter(Boolean);
    const deletedIdentifiers = new Set([
      ...successfullyDeletedIds.map((id) => `id:${id}`),
      ...localContactIdentifiers,
    ]);

    setContacts((currentContacts) =>
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

    setContacts((currentContacts) =>
      sortContacts(
        currentContacts.map((contact) => {
          const identifier = contactIdentifier(contact);
          if (!identifier || !identifiers.has(identifier)) return contact;
          return {
            ...contact,
            [field]: value,
            _status: contact._id === undefined ? 'Pending' : 'Updating',
            _error: undefined,
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
      <MainWindow
        settings={settings}
        log={log}
        radio={radio}
        isContextLoading={isContextLoading}
        contactsLoadState={contactsLoadState}
        stationCallsign={log?.station_callsign ?? ''}
        operatorCallsign={operatorCallsign}
        radioState={radioState}
        backendSocketStatus={backendSocketStatus}
        cwLabels={cwLabels}
        cwSentEvent={cwSentEvent}
        sessionId={sessionId}
        logId={numericLogId}
        onSetRadioFrequency={(frequencyHz) =>
          sendRadioMessage({ type: 'set_frequency', frequency_hz: frequencyHz })
        }
        onSetRadioMode={(mode) => sendRadioMessage({ type: 'set_mode', mode })}
        onSendCw={(payload) =>
          sendRadioMessage({ type: 'send_cw', ...payload })
        }
        onStopCw={() => sendRadioMessage({ type: 'stop_cw' })}
        onSetCwWpm={(wpm) => sendRadioMessage({ type: 'set_wpm', wpm })}
        onLogContact={(contact) =>
          setContacts((currentContacts) =>
            sortContacts([...currentContacts, contact]),
          )
        }
        onRescore={handleRescore}
        isRescoreLoading={isRescoreLoading}
        scoreSummary={scoreSummary}
        onExit={exitLogger}
      />
      <LogWindow
        settings={settings}
        contacts={contacts}
        log={log}
        contactsLoadState={contactsLoadState}
        radioMode={radioState?.mode ?? 'CW'}
        onDeleteContacts={deleteContacts}
        onUpdateContacts={updateContacts}
      />
    </div>
  );
}

export default LoggerScreen;
