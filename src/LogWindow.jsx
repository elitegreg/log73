import React from 'react';
import './App.css';

function LogWindow() {
  // Sample data mimicking the N1MM+ log window
  const logData = [
    {
      time: '12:00',
      freq: '14025',
      call: 'W1AW',
      rstSent: '59',
      rstRcvd: '59',
      nr: '001',
      mult1: 'CT',
      mult2: '',
      pts: '2',
      op: 'K1ZZ',
    },
    {
      time: '12:02',
      freq: '14027',
      call: 'K3LR',
      rstSent: '59',
      rstRcvd: '59',
      nr: '002',
      mult1: 'PA',
      mult2: '',
      pts: '2',
      op: 'K1ZZ',
    },
    {
      time: '12:05',
      freq: '14030',
      call: 'N2IC',
      rstSent: '59',
      rstRcvd: '59',
      nr: '003',
      mult1: 'NM',
      mult2: '',
      pts: '2',
      op: 'K1ZZ',
    },
    {
      time: '12:07',
      freq: '14028',
      call: 'VE3EJ',
      rstSent: '59',
      rstRcvd: '59',
      nr: '004',
      mult1: 'ON',
      mult2: '',
      pts: '4',
      op: 'K1ZZ',
    },
    {
      time: '12:10',
      freq: '14026',
      call: 'W4AN',
      rstSent: '59',
      rstRcvd: '59',
      nr: '005',
      mult1: 'GA',
      mult2: '',
      pts: '2',
      op: 'K1ZZ',
    },
  ];

  return (
    <div className="log-window">
      <div className="log-title-bar">Log: CQ WW SSB</div>
      <table className="log-table">
        <thead>
          <tr>
            <th>Time</th>
            <th>Freq</th>
            <th>Call</th>
            <th>RST S</th>
            <th>RST R</th>
            <th>Nr</th>
            <th>Mult1</th>
            <th>Mult2</th>
            <th>Pts</th>
            <th>Op</th>
          </tr>
        </thead>
        <tbody>
          {logData.map((entry, index) => (
            <tr key={index}>
              <td>{entry.time}</td>
              <td>{entry.freq}</td>
              <td>{entry.call}</td>
              <td>{entry.rstSent}</td>
              <td>{entry.rstRcvd}</td>
              <td>{entry.nr}</td>
              <td>{entry.mult1}</td>
              <td>{entry.mult2}</td>
              <td>{entry.pts}</td>
              <td>{entry.op}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default LogWindow;
