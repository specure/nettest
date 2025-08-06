import React from 'react';
import './SpeedTest.css';

const SpeedTest = ({ server, onError, onStartTest }) => {

  const handleStartTest = () => {
    onStartTest();
  };

  return (
    <div className="speed-test">
      <button onClick={handleStartTest} className="test-button">
        Start Nettest
      </button>


    </div>
  );
};

export default SpeedTest; 