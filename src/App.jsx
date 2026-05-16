import React, { useEffect, useRef, useState } from 'react';
import LogWindow from './LogWindow';
import MainWindow, { STATION_CALLSIGN } from './MainWindow';
import './App.css';

const BACKEND_HOST = window.location.hostname || '127.0.0.1';
const API_BASE_URL = `http://${BACKEND_HOST}:8080`;
const WS_BASE_URL = `${window.location.protocol === 'https:' ? 'wss' : 'ws'}://${BACKEND_HOST}:8080`;
const CONTACTS_STORAGE_KEY = 'log73.contacts';
const SESSION_STORAGE_KEY = 'log73.session_id';
const BACKEND_WS_INITIAL_RECONNECT_DELAY_MS = 2000;
const BACKEND_WS_MAX_RECONNECT_DELAY_MS = 16000;
const CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS = 2000;
const CONTACTS_LOAD_MAX_RETRY_DELAY_MS = 16000;

let promptedOperatorCallsign;

function promptForOperatorCallsign() {
  const defaultCallsign = promptedOperatorCallsign ?? STATION_CALLSIGN;
  const enteredCallsign = window.prompt('Operator Callsign', defaultCallsign) ?? '';
  promptedOperatorCallsign = enteredCallsign.toUpperCase();
  return promptedOperatorCallsign;
}

function getOperatorCallsign() {
  if (promptedOperatorCallsign === undefined) {
    return promptForOperatorCallsign();
  }

  return promptedOperatorCallsign;
}

function contactSortValue(contact) {
  if (typeof contact._time_on_epoch === 'number') {
    return contact._time_on_epoch;
  }

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

function sortContacts(contacts) {
  return [...contacts].sort((a, b) => contactSortValue(b) - contactSortValue(a));
}

function loadLocalContacts() {
  try {
    const parsed = JSON.parse(localStorage.getItem(CONTACTS_STORAGE_KEY) ?? '[]');
    return Array.isArray(parsed) ? sortContacts(parsed) : [];
  } catch (error) {
    console.error('Unable to load locally stored contacts', error);
    return [];
  }
}

function saveLocalContacts(contacts) {
  localStorage.setItem(CONTACTS_STORAGE_KEY, JSON.stringify(contacts));
}

function committedBackendContact(contact) {
  return { ...contact, _status: contact._status ?? 'Committed' };
}

function createSessionId() {
  if (window.crypto?.randomUUID) {
    return window.crypto.randomUUID();
  }

  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function getSessionId() {
  const existingSessionId = localStorage.getItem(SESSION_STORAGE_KEY);

  if (existingSessionId) {
    return existingSessionId;
  }

  const sessionId = createSessionId();
  localStorage.setItem(SESSION_STORAGE_KEY, sessionId);
  return sessionId;
}

function mergeContact(contacts, contact) {
  const committedContact = committedBackendContact(contact);
  const index = contacts.findIndex(
    (currentContact) =>
      currentContact._id && committedContact._id && currentContact._id === committedContact._id,
  );

  if (index === -1) {
    return sortContacts([...contacts, committedContact]);
  }

  const nextContacts = [...contacts];
  nextContacts[index] = { ...nextContacts[index], ...committedContact };
  return sortContacts(nextContacts);
}

function App() {
  const [settings, setSettings] = useState(null);
  const [contacts, setContacts] = useState(loadLocalContacts);
  const [operatorCallsign, setOperatorCallsign] = useState(getOperatorCallsign);
  const [sessionId] = useState(getSessionId);
  const [radioState, setRadioState] = useState({ mode: 'CW', frequency_hz: 14025000 });
  const [backendSocketStatus, setBackendSocketStatus] = useState('disconnected');
  const backendSocketRef = useRef(null);
  const committingContactIdsRef = useRef(new Set());

  useEffect(() => {
    saveLocalContacts(contacts);
  }, [contacts]);

  useEffect(() => {
    function handleKeyDown(event) {
      if (event.ctrlKey && !event.altKey && !event.metaKey && event.key.toLowerCase() === 'o') {
        event.preventDefault();
        setOperatorCallsign(promptForOperatorCallsign());
      }
    }

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  useEffect(() => {
    let shouldReconnect = true;
    let reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
    let reconnectTimerId;

    function scheduleReconnect() {
      if (!shouldReconnect || reconnectTimerId !== undefined) {
        return;
      }

      reconnectTimerId = window.setTimeout(() => {
        reconnectTimerId = undefined;
        connectBackendSocket();
      }, reconnectDelayMs);
      reconnectDelayMs = Math.min(reconnectDelayMs * 2, BACKEND_WS_MAX_RECONNECT_DELAY_MS);
    }

    function connectBackendSocket() {
      if (!shouldReconnect) {
        return;
      }

      const websocketUrl = `${WS_BASE_URL}/ws?session_id=${encodeURIComponent(sessionId)}`;
      console.info('Connecting backend websocket', websocketUrl);
      setBackendSocketStatus('connecting');
      const socket = new WebSocket(websocketUrl);
      backendSocketRef.current = socket;

      socket.addEventListener('open', () => {
        if (backendSocketRef.current !== socket) {
          return;
        }

        reconnectDelayMs = BACKEND_WS_INITIAL_RECONNECT_DELAY_MS;
        setBackendSocketStatus('connected');
      });

      socket.addEventListener('message', (event) => {
        if (backendSocketRef.current !== socket) {
          return;
        }

        try {
          const message = JSON.parse(event.data);
          if (message.type === 'radio_state') {
            setRadioState({
              frequency_hz: message.frequency_hz,
              mode: message.mode,
            });
          } else if (message.type === 'log_entry') {
            setContacts((currentContacts) => mergeContact(currentContacts, message.contact));
          }
        } catch (error) {
          console.error('Unable to process backend websocket message', error);
        }
      });

      socket.addEventListener('close', (event) => {
        console.info('Backend websocket closed', {
          code: event.code,
          reason: event.reason,
          wasClean: event.wasClean,
        });

        if (backendSocketRef.current === socket) {
          backendSocketRef.current = null;
          setBackendSocketStatus('disconnected');
          scheduleReconnect();
        }
      });

      socket.addEventListener('error', (error) => {
        if (backendSocketRef.current !== socket) {
          return;
        }

        console.error('Backend websocket error', error);
        setBackendSocketStatus('disconnected');
        socket.close();
      });
    }

    connectBackendSocket();

    return () => {
      shouldReconnect = false;
      if (reconnectTimerId !== undefined) {
        window.clearTimeout(reconnectTimerId);
      }
      const socket = backendSocketRef.current;
      backendSocketRef.current = null;
      socket?.close();
    };
  }, [sessionId]);

  useEffect(() => {
    let shouldRetryContactsLoad = true;
    let contactsLoadRetryDelayMs = CONTACTS_LOAD_INITIAL_RETRY_DELAY_MS;
    let contactsLoadRetryTimerId;

    async function loadContestSettings() {
      try {
        const settingsResponse = await fetch(
          `${API_BASE_URL}/contest-settings/get`,
        );
        if (!settingsResponse.ok) {
          throw new Error(
            `contest settings request failed: ${settingsResponse.status}`,
          );
        }
        const contestSettings = await settingsResponse.json();
        setSettings(contestSettings);
      } catch (error) {
        alert(
          `Unable to load contest settings from the backend.\n\n${error.message}`,
        );
      }
    }

    function scheduleContactsLoadRetry() {
      if (!shouldRetryContactsLoad || contactsLoadRetryTimerId !== undefined) {
        return;
      }

      contactsLoadRetryTimerId = window.setTimeout(() => {
        contactsLoadRetryTimerId = undefined;
        loadContacts();
      }, contactsLoadRetryDelayMs);
      contactsLoadRetryDelayMs = Math.min(
        contactsLoadRetryDelayMs * 2,
        CONTACTS_LOAD_MAX_RETRY_DELAY_MS,
      );
    }

    async function loadContacts() {
      try {
        const contactsResponse = await fetch(`${API_BASE_URL}/contacts`);
        if (!contactsResponse.ok) {
          throw new Error(
            `contacts request failed: ${contactsResponse.status}`,
          );
        }
        const backendContacts = (await contactsResponse.json()).map(committedBackendContact);
        const localUncommittedContacts = loadLocalContacts().filter(
          (contact) => contact._status !== 'Committed',
        );
        setContacts(sortContacts([...backendContacts, ...localUncommittedContacts]));
        shouldRetryContactsLoad = false;
      } catch (error) {
        console.error('Unable to load backend contacts; using local contacts', error);
        scheduleContactsLoadRetry();
      }
    }

    loadContestSettings();
    loadContacts();

    return () => {
      shouldRetryContactsLoad = false;
      if (contactsLoadRetryTimerId !== undefined) {
        window.clearTimeout(contactsLoadRetryTimerId);
      }
    };
  }, []);

  useEffect(() => {
    const pendingContact = contacts.find(
      (contact) =>
        contact._status === 'Pending' &&
        contact._id &&
        !committingContactIdsRef.current.has(contact._id),
    );

    if (!pendingContact) {
      return;
    }

    committingContactIdsRef.current.add(pendingContact._id);

    async function commitContact(contact) {
      try {
        const response = await fetch(`${API_BASE_URL}/contacts`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(contact),
        });

        if (!response.ok) {
          throw new Error(`commit failed: ${response.status}`);
        }

        const responseBody = await response.json();
        if (responseBody.contact) {
          setContacts((currentContacts) => mergeContact(currentContacts, responseBody.contact));
        }
      } catch (error) {
        console.error('Unable to commit contact', error);
        window.setTimeout(() => {
          setContacts((currentContacts) => sortContacts(currentContacts));
        }, 5000);
      } finally {
        committingContactIdsRef.current.delete(contact._id);
      }
    }

    commitContact(pendingContact);
  }, [contacts]);

  function sendRadioMessage(message) {
    const socket = backendSocketRef.current;
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify(message));
    }
  }

  function setRadioFrequency(frequencyHz) {
    sendRadioMessage({ type: 'set_frequency', frequency_hz: frequencyHz });
  }

  function setRadioMode(mode) {
    sendRadioMessage({ type: 'set_mode', mode });
  }

  function addContact(contact) {
    setContacts((currentContacts) => sortContacts([...currentContacts, contact]));
  }

  return (
    <div className="app-container">
      <MainWindow
        settings={settings}
        operatorCallsign={operatorCallsign}
        radioState={radioState}
        backendSocketStatus={backendSocketStatus}
        sessionId={sessionId}
        onSetRadioFrequency={setRadioFrequency}
        onSetRadioMode={setRadioMode}
        onLogContact={addContact}
      />
      <LogWindow settings={settings} contacts={contacts} />
    </div>
  );
}

export default App;
