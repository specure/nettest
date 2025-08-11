import React, { useState, useEffect } from 'react';
import './App.css';
import SpeedTest from './components/SpeedTest';
import ServerSelect from './components/ServerSelect';
import TestModal from './components/TestModal';
import ServersMap from './components/ServersMap';
import QuickActions from './components/QuickActions';
import TestResults from './components/TestResults';

function App() {
  const [servers, setServers] = useState([]);
  const [selectedServer, setSelectedServer] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isTestResultsOpen, setIsTestResultsOpen] = useState(false);

  useEffect(() => {
    loadServers();
  }, []);

  const loadServers = async () => {
    try {
      setLoading(true);
      const response = await fetch('https://api.nettest.org/measurementServer', {
        headers: {
          'Content-Type': 'application/json',
          'X-Nettest-Client': 'nt'
        }
      });
      if (!response.ok) {
        throw new Error('Failed to fetch servers');
      }
      const data = await response.json();
      setServers(data);
      
      // Auto-select the nearest server (lowest distance)
      if (data && data.length > 0) {
        const nearestServer = data.reduce((nearest, current) => {
          // Skip servers without distance
          if (!current.distance || current.distance === 0) return nearest;
          if (!nearest.distance || nearest.distance === 0) return current;
          
          return current.distance < nearest.distance ? current : nearest;
        });
        
        if (nearestServer && nearestServer.distance) {
          console.log('Auto-selecting nearest server:', nearestServer.name, 'distance:', nearestServer.distance);
          setSelectedServer(nearestServer);
        }
      }
    } catch (err) {
      console.error('Error loading servers:', err);
      setError('Failed to load servers. Please try again later.');
    } finally {
      setLoading(false);
    }
  };

  const handleServerSelect = (server) => {
    console.log('Server selected:', server);
    setSelectedServer(server);
    setError(null);
  };

  const handleServerSelectFromMap = (server) => {
    console.log('handleServerSelectFromMap called with:', server);
    setSelectedServer(server);
    setError(null);
    
    // Automatically start measurement after a short delay
    setTimeout(() => {
      console.log('Starting measurement for server:', server);
      setIsModalOpen(true);
    }, 800);
  };

  return (
    <div className="App">
      <div className="App-header">
        <h1 className="App-title text-glow">Nettest</h1>
        <p className="App-subtitle">Measure your internet connection performance</p>
      </div>
      
      <div className="App-content">
        {error && <div className="error status">{error}</div>}
        
        {servers.length > 0 ? (
          <>
            {/* Servers Map */}
            <div className="card">
              <h2 className="card-title">Measurement Servers</h2>
              <div className="card-content">
                <ServersMap 
                  servers={servers}
                  onServerSelect={handleServerSelectFromMap}
                />
              </div>
            </div>

            {/* Test Controls */}
            <div className="card">
              <div className="card-content">
                <div className="test-controls">
                  <ServerSelect
                    servers={servers}
                    selectedServer={selectedServer}
                    onServerSelect={handleServerSelect}
                    loading={loading}
                    highlightSelected={selectedServer && selectedServer.id}
                  />
                  
                  {selectedServer && (
                    <SpeedTest
                      server={selectedServer}
                      onError={setError}
                      onStartTest={() => setIsModalOpen(true)}
                    />
                  )}
                </div>
              </div>
            </div>

            {/* Test Results Button */}
            <div className="card">
              <div className="card-content">
                <button 
                  className="test-results-btn"
                  onClick={() => setIsTestResultsOpen(true)}
                >
                  📊 View Test Results History
                </button>
              </div>
            </div>
          </>
        ) : (
          <div className="card">
            <div className="card-content">
              <div className="status">Loading servers...</div>
            </div>
          </div>
        )}
      </div>

      <TestModal
        server={selectedServer}
        isOpen={isModalOpen}
        onClose={() => setIsModalOpen(false)}
      />
      
      <TestResults
        isOpen={isTestResultsOpen}
        onClose={() => setIsTestResultsOpen(false)}
      />
      
      <QuickActions />
    </div>
  );
}

export default App; 