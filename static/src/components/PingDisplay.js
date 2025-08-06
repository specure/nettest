import React from 'react';
import './PingDisplay.css';

const PingDisplay = ({ pingMedian, isActive }) => {
  console.log('=== PingDisplay render ===');
  console.log('pingMedian:', pingMedian);
  console.log('isActive:', isActive);
  console.log('pingMedian type:', typeof pingMedian);

  // Показываем placeholder только если нет данных пинга И тест не активен
  if (!isActive && !pingMedian) {
    console.log('PingDisplay: showing placeholder (not active and no data)');
    return (
      <div className="ping-display-container">
        <h3>Ping</h3>
        <div className="ping-display-placeholder">
          Chart will appear when test starts
        </div>
      </div>
    );
  }

  console.log('PingDisplay: rendering with pingMedian:', pingMedian);
  return (
    <div className="ping-display-container">
      <h3>Ping</h3>
      <div className="ping-value">
        {pingMedian ? (
          <>
            <span className="ping-number">{pingMedian}</span>
            <span className="ping-unit">ms</span>
          </>
        ) : (
          <span className="ping-waiting">Measuring...</span>
        )}
      </div>
    </div>
  );
};

export default PingDisplay; 