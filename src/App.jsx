import React, { useEffect, useState } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';
import CreateLogScreen from './CreateLogScreen';
import CreateRadioScreen from './CreateRadioScreen';
import LoggerScreen from './LoggerScreen';
import OpenLogScreen from './OpenLogScreen';
import { loadTheme, themeClassName, THEME_CLASS_NAMES, THEME_STORAGE_KEY } from './themes';
import './App.css';
import './theme-modern-dark-radio.css';
import './theme-classic-terminal.css';
import './theme-clean-light-desktop.css';
import './theme-n1mm-contest.css';
import './theme-high-contrast.css';

function App() {
  const [theme, setTheme] = useState(loadTheme);

  useEffect(() => {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
    document.body.classList.remove(...THEME_CLASS_NAMES);
    const nextThemeClassName = themeClassName(theme);
    if (nextThemeClassName) document.body.classList.add(nextThemeClassName);
  }, [theme]);

  return (
    <div className="app-container">
      <Routes>
        <Route path="/" element={<Navigate to="/ui/open_log" replace />} />
        <Route path="/ui/open_log" element={<OpenLogScreen theme={theme} onSetTheme={setTheme} />} />
        <Route path="/ui/create_log" element={<CreateLogScreen />} />
        <Route path="/ui/create_radio" element={<CreateRadioScreen />} />
        <Route path="/ui/logger/:logId/:radioId" element={<LoggerScreen />} />
        <Route path="*" element={<Navigate to="/ui/open_log" replace />} />
      </Routes>
    </div>
  );
}

export default App;
