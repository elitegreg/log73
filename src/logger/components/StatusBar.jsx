import React from 'react';

function StatusBar({ stationCallsign, operatorCallsign, scoreSummary }) {
  return (
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
  );
}

export default StatusBar;
