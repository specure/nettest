import React, { useState } from 'react';
import './SpeedTest.css';
import TestModal from './TestModal';

const SpeedTest = ({ server, onError }) => {
  const [isModalOpen, setIsModalOpen] = useState(false);

  const handleStartTest = () => {
    setIsModalOpen(true);
  };

  const handleCloseModal = () => {
    setIsModalOpen(false);
  };

  return (
    <div className="speed-test">
      <button onClick={handleStartTest} className="test-button">
        Start Speed Test
      </button>

      <TestModal
        server={server}
        isOpen={isModalOpen}
        onClose={handleCloseModal}
      />
    </div>
  );
};

export default SpeedTest; 