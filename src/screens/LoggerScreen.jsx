import React, { useEffect, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { apiJson, websocketUrl } from '../lib/api';
import LogWindow from '../logger/LogWindow';
import MainWindow from '../logger/MainWindow';

const CONTACTS_STORAGE_KEY = 'log73.contacts';
const SESSION_STORAGE_KEY = 'log73.session_id';
const BACKEND_WS_INITIAL_RECONNECT_DELAY_MS = 2000;
const BACKEND_WS_MAX_RECONNECT_DELAY_MS = 16000;
const CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS = 2000;
const CONTACTS_LOAD_MAX_RETRY_DELAY_MS = 16000;

let promptedOperatorCallsign;

function promptForOperatorCallsign(defaultCallsign) {
  const enteredCallsign = window.prompt('Operator Callsign', promptedOperatorCallsign ?? defaultCallsign);
  if (enteredCallsign === null) return promptedOperatorCallsign ?? defaultCallsign;
  promptedOperatorCallsign = enteredCallsign.toUpperCase();
  return promptedOperatorCallsign;
}

function contactSortValue(contact) {
  if (typeof contact.QSO_DATE_TIME_ON === 'number') return contact.QSO_DATE_TIME_ON;
  if (typeof contact._time_on_epoch === 'number') return contact._time_on_epoch;
  const date = String(contact.QSO_DATE ?? '');
  const time = String(contact.TIME_ON ?? '');
  const parsed = Date.UTC(
    Number.parseInt(date.slice(0, 4), 10),
    Number.parseInt(date.slice(4, 6), 10) - 1,
    Number.parseInt(date.slice(6, 8), 10),
    Number.parseInt(time.slice(0, 2), 10),
    Number.parseInt(time.slice(2, 4), 10),
    Number.parseInt(time.slice(4, 6), 10),
  );
  return Number.isFinite(parsed) ? Math.floor(parsed / 1000) : 0;
}

function sortContacts(contacts) { return [...contacts].sort((a, b) => contactSortValue(b) - contactSortValue(a)); }

function normalizeContact(contact) {
  const nextContact = { ...contact };
  if (nextContact._status === 'Committed') delete nextContact._client_id;
  if (typeof nextContact.QSO_DATE_TIME_ON !== 'number') {
    const epoch = contactSortValue(nextContact);
    if (epoch > 0) nextContact.QSO_DATE_TIME_ON = epoch;
  }
  if (nextContact.FREQ !== undefined) {
    const frequency = Number.parseFloat(String(nextContact.FREQ));
    if (Number.isFinite(frequency)) nextContact.FREQ = Math.round(Math.abs(frequency) < 1000000 ? frequency * 1000000 : frequency);
  }
  delete nextContact.QSO_DATE;
  delete nextContact.TIME_ON;
  delete nextContact._time_on_epoch;
  return nextContact;
}

function shouldPersistLocally(contact) { return contact._status === 'Pending' || contact._status === 'Updating'; }
function contactStorageKey(logId) { return `${CONTACTS_STORAGE_KEY}.${logId}`; }

function loadLocalContacts(logId) {
  try {
    const parsed = JSON.parse(localStorage.getItem(contactStorageKey(logId)) ?? '[]');
    return Array.isArray(parsed) ? sortContacts(parsed.map(normalizeContact).filter(shouldPersistLocally)) : [];
  } catch (error) {
    console.error('Unable to load locally stored contacts', error);
    return [];
  }
}

function saveLocalContacts(logId, contacts) {
  localStorage.setItem(contactStorageKey(logId), JSON.stringify(contacts.filter(shouldPersistLocally)));
}

function committedBackendContact(contact) { return normalizeContact({ ...contact, _status: contact._status ?? 'Committed' }); }
function createSessionId() { return window.crypto?.randomUUID ? window.crypto.randomUUID() : `${Date.now()}-${Math.random().toString(36).slice(2)}`; }
function getSessionId() {
  const existingSessionId = localStorage.getItem(SESSION_STORAGE_KEY);
  if (existingSessionId) return existingSessionId;
  const sessionId = createSessionId();
  localStorage.setItem(SESSION_STORAGE_KEY, sessionId);
  return sessionId;
}

function contactMatches(left, right) {
  if (left._id !== undefined && right._id !== undefined) return String(left._id) === String(right._id);
  if (left._client_id && right._client_id) return left._client_id === right._client_id;
  return false;
}

function contactIdentifier(contact) {
  if (contact._id !== undefined) return `id:${contact._id}`;
  if (contact._client_id) return `client:${contact._client_id}`;
  return null;
}

function mergeContact(contacts, contact) {
  const committedContact = committedBackendContact(contact);
  const index = contacts.findIndex((currentContact) => contactMatches(currentContact, contact));
  if (index === -1) return sortContacts([...contacts, committedContact]);
  const nextContacts = [...contacts];
  nextContacts[index] = { ...nextContacts[index], ...committedContact };
  return sortContacts(nextContacts);
}

function LoggerScreen() {
  const { logId, radioId } = useParams();
  const navigate = useNavigate();
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
  const [radioState, setRadioState] = useState({ mode: 'CW', frequency_hz: 14025000 });
  const [backendSocketStatus, setBackendSocketStatus] = useState('disconnected');
  const [scoreSummary, setScoreSummary] = useState({ qsoCount: 0, multipliers: 0, bonusPoints: 0, score: 0 });
  const backendSocketRef = useRef(null);
  const committingContactIdsRef = useRef(new Set());
  const refreshContactsRef = useRef(() => {});

  useEffect(() => { saveLocalContacts(logId, contacts); }, [contacts, logId]);

  useEffect(() => {
    setScoreSummary({ qsoCount: 0, multipliers: 0, bonusPoints: 0, score: 0 });
  }, [numericLogId]);

  useEffect(() => {
    async function loadContext() {
      const [logResult, radioResult, cwLabelsResult] = await Promise.all([
        apiJson(`/logs/${numericLogId}`),
        apiJson(`/radios/${numericRadioId}`),
        apiJson(`/radios/${numericRadioId}/cw-labels`),
      ]);
      if (!logResult.ok) throw new Error(logResult.error ?? 'Log not found');
      if (!radioResult.ok) throw new Error(radioResult.error ?? 'Radio not found');
      const contestSettings = await apiJson(`/contest-settings?contest_id=${encodeURIComponent(logResult.log.contest_id)}`);
      setSettings(contestSettings);
      setLog(logResult.log);
      setRadio(radioResult.radio);
      if (cwLabelsResult.ok) setCwLabels(cwLabelsResult.labels);
      setOperatorCallsign((current) => current || promptForOperatorCallsign(logResult.log.station_callsign));
    }
    loadContext().catch((error) => alert(`Unable to load logger context.\n\n${error.message}`));
  }, [numericLogId, numericRadioId]);

  useEffect(() => {
    function handleKeyDown(event) {
      if (event.ctrlKey && !event.altKey && !event.metaKey && event.key.toLowerCase() === 'o') {
        event.preventDefault();
        setOperatorCallsign(promptForOperatorCallsign(log?.station_callsign ?? ''));
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
      reconnectDelayMs = Math.min(reconnectDelayMs * 2, BACKEND_WS_MAX_RECONNECT_DELAY_MS);
    }

    function connectBackendSocket() {
      if (!shouldReconnect) return;
      const url = websocketUrl({ session_id: sessionId, log_id: numericLogId, radio_id: numericRadioId });
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
            setRadioState({ frequency_hz: message.frequency_hz, mode: message.mode });
          } else if (message.type === 'cw_sent') {
            setCwSentEvent({ requestId: message.request_id, sequence: Date.now() });
          } else if (message.type === 'log_entry' && message.contact?._session_id !== sessionId && Number(message.contact?._log_id) === numericLogId) {
            setContacts((currentContacts) => mergeContact(currentContacts, message.contact));
          } else if (message.type === 'contact_deleted' && Number(message.log_id) === numericLogId) {
            setContacts((currentContacts) => currentContacts.filter((contact) => String(contact._id) !== String(message.id)));
          } else if (message.type === 'score_update' && Number(message.log_id) === numericLogId) {
            setScoreSummary({
              qsoCount: Number(message.qso_count ?? 0),
              multipliers: Number(message.multipliers ?? 0),
              bonusPoints: Number(message.bonus_points ?? 0),
              score: Number(message.total_score ?? 0),
            });
          }
        } catch (error) {
          console.error('Unable to process backend websocket message', error);
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

    function scheduleContactsLoadRetry() {
      if (!shouldRetryContactsLoad || contactsLoadRetryTimerId !== undefined) return;
      contactsLoadRetryTimerId = window.setTimeout(() => {
        contactsLoadRetryTimerId = undefined;
        loadContacts();
      }, contactsLoadRetryDelayMs);
      contactsLoadRetryDelayMs = Math.min(contactsLoadRetryDelayMs * 2, CONTACTS_LOAD_MAX_RETRY_DELAY_MS);
    }

    async function loadContacts() {
      try {
        const backendContacts = (await apiJson(`/logs/${numericLogId}/contacts`)).map(committedBackendContact);
        const localUncommittedContacts = loadLocalContacts(logId).filter((contact) => contact._status !== 'Committed');
        setContacts(sortContacts([...backendContacts, ...localUncommittedContacts]));
        shouldRetryContactsLoad = false;
      } catch (error) {
        console.error('Unable to load backend contacts; using local contacts', error);
        scheduleContactsLoadRetry();
      }
    }

    refreshContactsRef.current = loadContacts;
    loadContacts();
    return () => {
      refreshContactsRef.current = () => {};
      shouldRetryContactsLoad = false;
      if (contactsLoadRetryTimerId !== undefined) window.clearTimeout(contactsLoadRetryTimerId);
    };
  }, [numericLogId, logId]);

  useEffect(() => {
    const pendingContact = contacts.find((contact) => {
      if (contact._status === 'Pending') return contact._client_id && !committingContactIdsRef.current.has(contact._client_id);
      if (contact._status === 'Updating') {
        const updateKey = contact._id ?? contact._client_id;
        return updateKey && !committingContactIdsRef.current.has(updateKey);
      }
      return false;
    });
    if (!pendingContact) return;

    const commitKey = pendingContact._status === 'Pending' ? pendingContact._client_id : pendingContact._id ?? pendingContact._client_id;
    committingContactIdsRef.current.add(commitKey);

    async function commitContact(contact) {
      try {
        const responseBody = await apiJson(`/logs/${numericLogId}/contacts`, {
          method: 'POST',
          body: JSON.stringify({ ...contact, _log_id: numericLogId }),
        });
        if (responseBody.contact) {
          setContacts((currentContacts) => mergeContact(currentContacts, { ...responseBody.contact, _client_id: contact._client_id }));
        }
      } catch (error) {
        console.error('Unable to commit contact', error);
        window.setTimeout(() => setContacts((currentContacts) => sortContacts(currentContacts)), 5000);
      } finally {
        committingContactIdsRef.current.delete(commitKey);
      }
    }

    commitContact(pendingContact);
  }, [contacts, numericLogId]);

  function sendRadioMessage(message) {
    const socket = backendSocketRef.current;
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(message));
  }

  async function deleteContacts(contactsToDelete) {
    if (contactsToDelete.length === 0) return;

    const qsoLabel = contactsToDelete.length === 1 ? '1 QSO' : `${contactsToDelete.length} QSOs`;
    if (!window.confirm(`Are you sure you want to delete ${qsoLabel}?`)) return;

    const committedContacts = contactsToDelete.filter((contact) => contact._id !== undefined);
    const localContactIdentifiers = contactsToDelete
      .filter((contact) => contact._id === undefined)
      .map(contactIdentifier)
      .filter(Boolean);
    const successfullyDeletedIds = [];
    const results = await Promise.allSettled(
      committedContacts.map(async (contact) => {
        const result = await apiJson(`/contacts/${contact._id}`, { method: 'DELETE' });
        if (!result.ok) throw new Error(result.error ?? 'Unable to delete contact');
        if (result.deleted) successfullyDeletedIds.push(String(contact._id));
      }),
    );
    const failureCount = results.filter((result) => result.status === 'rejected').length;
    const deletedIdentifiers = new Set([
      ...successfullyDeletedIds.map((id) => `id:${id}`),
      ...localContactIdentifiers,
    ]);

    setContacts((currentContacts) => currentContacts.filter((contact) => {
      const identifier = contactIdentifier(contact);
      return !identifier || !deletedIdentifiers.has(identifier);
    }));

    if (failureCount > 0) {
      window.alert(`Unable to delete ${failureCount === 1 ? '1 QSO' : `${failureCount} QSOs`}.`);
    }
  }

  function updateContacts(contactsToUpdate, field, value) {
    const identifiers = new Set(contactsToUpdate.map(contactIdentifier).filter(Boolean));
    if (identifiers.size === 0) return;

    setContacts((currentContacts) => sortContacts(currentContacts.map((contact) => {
      const identifier = contactIdentifier(contact);
      if (!identifier || !identifiers.has(identifier)) return contact;
      return {
        ...contact,
        [field]: value,
        _status: contact._status === 'Pending' ? 'Pending' : 'Updating',
      };
    })));
  }

  function exitLogger() { navigate('/ui/open_log'); }

  return (
    <div className="app-container">
      <MainWindow
        settings={settings}
        log={log}
        radio={radio}
        stationCallsign={log?.station_callsign ?? ''}
        operatorCallsign={operatorCallsign}
        radioState={radioState}
        backendSocketStatus={backendSocketStatus}
        cwLabels={cwLabels}
        cwSentEvent={cwSentEvent}
        sessionId={sessionId}
        logId={numericLogId}
        onSetRadioFrequency={(frequencyHz) => sendRadioMessage({ type: 'set_frequency', frequency_hz: frequencyHz })}
        onSetRadioMode={(mode) => sendRadioMessage({ type: 'set_mode', mode })}
        onSendCw={(payload) => sendRadioMessage({ type: 'send_cw', ...payload })}
        onStopCw={() => sendRadioMessage({ type: 'stop_cw' })}
        onSetCwWpm={(wpm) => sendRadioMessage({ type: 'set_wpm', wpm })}
        onLogContact={(contact) => setContacts((currentContacts) => sortContacts([...currentContacts, contact]))}
        onRescore={() => refreshContactsRef.current()}
        scoreSummary={scoreSummary}
        onExit={exitLogger}
      />
      <LogWindow
        settings={settings}
        contacts={contacts}
        log={log}
        radioMode={radioState?.mode ?? 'CW'}
        onDeleteContacts={deleteContacts}
        onUpdateContacts={updateContacts}
      />
    </div>
  );
}

export default LoggerScreen;
