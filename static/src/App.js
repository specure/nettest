import React, { useState, useEffect } from 'react';
import './App.css';
import SpeedTest from './components/SpeedTest';
import ServerSelect from './components/ServerSelect';
import TestModal from './components/TestModal';
import ServersMap from './components/ServersMap';

function App() {
  const [servers, setServers] = useState([]);
  const [selectedServer, setSelectedServer] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [isModalOpen, setIsModalOpen] = useState(false);

  useEffect(() => {
    loadServers();
  }, []);

  const loadServers = async () => {
    try {
      setLoading(true);
      const response = await fetch('http://localhost:3001/api/servers');
      if (!response.ok) {
        throw new Error('Failed to fetch servers');
      }
      const data = await response.json();
      setServers(data);
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
              <h2 className="card-title">Start Measurement</h2>
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
    </div>
  );
}

export default App; 