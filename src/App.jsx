import React from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';
import CreateLogScreen from './CreateLogScreen';
import CreateRadioScreen from './CreateRadioScreen';
import LoggerScreen from './LoggerScreen';
import OpenLogScreen from './OpenLogScreen';
import './App.css';

function App() {
  return (
    <div className="app-container">
      <Routes>
        <Route path="/" element={<Navigate to="/ui/open_log" replace />} />
        <Route path="/ui/open_log" element={<OpenLogScreen />} />
        <Route path="/ui/create_log" element={<CreateLogScreen />} />
        <Route path="/ui/create_radio" element={<CreateRadioScreen />} />
        <Route path="/ui/logger/:logId/:radioId" element={<LoggerScreen />} />
        <Route path="*" element={<Navigate to="/ui/open_log" replace />} />
      </Routes>
    </div>
  );
}

export default App;
