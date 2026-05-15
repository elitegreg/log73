import React, { useEffect, useState } from 'react';
import LogWindow from './LogWindow';
import MainWindow, { STATION_CALLSIGN } from './MainWindow';
import './App.css';

const API_BASE_URL = `http://${window.location.hostname || '127.0.0.1'}:8080`;

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

  return (
    <div className="app-container">
      <MainWindow settings={settings} operatorCallsign={operatorCallsign} />
      <LogWindow settings={settings} contacts={contacts} />
    </div>
  );
}

export default App;
