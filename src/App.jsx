import React, { useEffect, useRef, useState } from 'react';
import LogWindow from './LogWindow';
import MainWindow, { STATION_CALLSIGN } from './MainWindow';
import './App.css';

const BACKEND_HOST = window.location.hostname || '127.0.0.1';
const API_BASE_URL = `http://${BACKEND_HOST}:8080`;
const WS_BASE_URL = `${window.location.protocol === 'https:' ? 'wss' : 'ws'}://${BACKEND_HOST}:8080`;

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

function App() {
  const [settings, setSettings] = useState(null);
  const [contacts, setContacts] = useState([]);
  const [operatorCallsign, setOperatorCallsign] = useState(getOperatorCallsign);
  const [radioState, setRadioState] = useState({ mode: 'CW', frequency_hz: 14025000 });
  const radioSocketRef = useRef(null);

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
    const socket = new WebSocket(`${WS_BASE_URL}/ws`);
    radioSocketRef.current = socket;

    socket.addEventListener('message', (event) => {
      try {
        const message = JSON.parse(event.data);
        if (message.type === 'radio_state') {
          setRadioState({
            frequency_hz: message.frequency_hz,
            mode: message.mode,
          });
        }
      } catch (error) {
        console.error('Unable to process radio websocket message', error);
      }
    });

    socket.addEventListener('close', () => {
      if (radioSocketRef.current === socket) {
        radioSocketRef.current = null;
      }
    });

    socket.addEventListener('error', (error) => {
      console.error('Radio websocket error', error);
    });

    return () => {
      radioSocketRef.current = null;
      socket.close();
    };
  }, []);

  useEffect(() => {
    async function loadContest() {
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

        const contactsResponse = await fetch(`${API_BASE_URL}/contacts/get`);
        if (!contactsResponse.ok) {
          throw new Error(
            `contacts request failed: ${contactsResponse.status}`,
          );
        }
        setContacts(await contactsResponse.json());
      } catch (error) {
        alert(
          `Unable to load contest data from the backend.\n\n${error.message}`,
        );
      }
    }

    loadContest();
  }, []);

  function sendRadioMessage(message) {
    const socket = radioSocketRef.current;
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

  return (
    <div className="app-container">
      <MainWindow
        settings={settings}
        operatorCallsign={operatorCallsign}
        radioState={radioState}
        onSetRadioFrequency={setRadioFrequency}
        onSetRadioMode={setRadioMode}
      />
      <LogWindow settings={settings} contacts={contacts} />
    </div>
  );
}

export default App;
