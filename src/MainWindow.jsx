import React from 'react';
import './App.css';

function MainWindow() {
  return (
    <div className="window">
      <div className="title-bar">Contact Mode (LSB) &lt; Contest mode CW !</div>
      <div className="menu-bar">
        <span>File</span>
        <span>Edit</span>
        <span>View</span>
        <span>Tools</span>
        <span>Config</span>
        <span>Window</span>
        <span>Help</span>
      </div>
      <div className="entry-fields">
        <input type="text" placeholder="Callsign" className="callsign" />
        <input type="text" placeholder="RST(s)" className="report" />
        <input type="text" placeholder="RST(r)" className="report" />
        <input type="text" placeholder="Exchange" className="exchange" />
      </div>
      <div className="function-keys">
        <div className="f-row">
          <button className="f-key active">F1 S&amp;P CQ</button>
          <button className="f-key">F2 Exch</button>
          <button className="f-key">F3 Spare</button>
          <button className="f-key">F4 KBUT</button>
          <button className="f-key">F5 His Call</button>
          <button className="f-key">F6 KBUT</button>
        </div>
        <div className="f-row">
          <button className="f-key">F7 Rpt Exch</button>
          <button className="f-key">F8 Agn?</button>
          <button className="f-key">F9 Zone</button>
          <button className="f-key">F10 Spare</button>
          <button className="f-key">F11 Spare</button>
          <button className="f-key">F12 Wipe</button>
        </div>
      </div>
      <div className="command-buttons">
        <button className="cmd-btn">Call Esc Stop</button>
        <button className="cmd-btn">Wipe</button>
        <button className="cmd-btn">UserText</button>
        <button className="cmd-btn">Log it</button>
        <button className="cmd-btn">Edit</button>
        <button className="cmd-btn">Mark</button>
        <button className="cmd-btn">Store</button>
        <button className="cmd-btn">Spot It</button>
        <button className="cmd-btn">QRZ</button>
      </div>
      <div className="history">Call history appears here when enabled.</div>
      <div className="status-bar">
        <span>KN4YED UNITED STATES Zn5</span>
        <span>4.0</span>
        <span>0</span>
      </div>
    </div>
  );
}

export default MainWindow;
