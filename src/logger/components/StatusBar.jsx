import React from 'react';
import { formatQsoRate } from '../../screens/loggerScreen/qsoStatsState.js';

const RATE_COLUMNS = [
  ['last_10_contacts', '10-QSO'],
  ['last_100_contacts', '100-QSO'],
  ['moving_hour', 'Last hour'],
  ['clock_hour', 'Clock hour'],
];

function RateRow({ label, stats }) {
  return (
    <tr>
      <th scope="row">{label}</th>
      {RATE_COLUMNS.map(([key]) => (
        <td key={key}>{formatQsoRate(stats?.[key])}</td>
      ))}
    </tr>
  );
}

function StatusBar({
  stationCallsign,
  operatorCallsign,
  scoreSummary,
  overallQsoStats,
  operatorQsoStats,
}) {
  return (
    <div className="status-and-rates">
      <div className="status-bar">
        <span>
          {stationCallsign} / Op: {operatorCallsign}
        </span>
        <span>
          QSOs: {scoreSummary?.qsoCount ?? 0}
          {scoreSummary?.multipliers
            ? `  Mults: ${scoreSummary.multipliers}`
            : ''}
          {scoreSummary?.bonusPoints
            ? `  Bonus: ${scoreSummary.bonusPoints}`
            : ''}{' '}
          Score: {scoreSummary?.score ?? 0}
        </span>
      </div>
      <table className="qso-rate-table" aria-label="QSO rates per hour">
        <thead>
          <tr>
            <th aria-label="Scope" />
            {RATE_COLUMNS.map(([key, label]) => (
              <th key={key} scope="col">
                {label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          <RateRow label="This operator" stats={operatorQsoStats} />
          <RateRow label="All operators" stats={overallQsoStats} />
        </tbody>
      </table>
    </div>
  );
}

export default StatusBar;
