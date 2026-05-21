import React, { useEffect, useState } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';
import ConfigScreen from '../screens/ConfigScreen';
import CreateLogScreen from '../screens/CreateLogScreen';
import CreateRadioScreen from '../screens/CreateRadioScreen';
import LoggerScreen from '../screens/LoggerScreen';
import OpenLogScreen from '../screens/OpenLogScreen';
import {
  loadTheme,
  themeClassName,
  THEME_CLASS_NAMES,
  THEME_STORAGE_KEY,
} from '../themes/themes';
import '../styles/App.css';
import '../styles/theme-modern-dark-radio.css';
import '../styles/theme-classic-terminal.css';
import '../styles/theme-clean-light-desktop.css';
import '../styles/theme-n1mm-contest.css';
import '../styles/theme-high-contrast.css';

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
        <Route
          path="/ui/open_log"
          element={<OpenLogScreen theme={theme} onSetTheme={setTheme} />}
        />
        <Route
          path="/ui/config"
          element={<ConfigScreen theme={theme} onSetTheme={setTheme} />}
        />
        <Route path="/ui/create_log" element={<CreateLogScreen />} />
        <Route path="/ui/edit_log/:logId" element={<CreateLogScreen />} />
        <Route path="/ui/create_radio" element={<CreateRadioScreen />} />
        <Route path="/ui/edit_radio/:radioId" element={<CreateRadioScreen />} />
        <Route path="/ui/logger/:logId/:radioId" element={<LoggerScreen />} />
        <Route path="*" element={<Navigate to="/ui/open_log" replace />} />
      </Routes>
    </div>
  );
}

export default App;
