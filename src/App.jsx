import React from 'react';
import LogWindow from './LogWindow';
import MainWindow from './MainWindow';
import './App.css';

function App() {
  return (
    <div className="app-container">
      <MainWindow />
      <LogWindow />
    </div>
  );
}

export default App;
