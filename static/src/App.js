import React, { useState, useEffect } from 'react';
import './App.css';
import SpeedTest from './components/SpeedTest';
import ServerSelect from './components/ServerSelect';

function App() {
  const [servers, setServers] = useState([]);
  const [selectedServer, setSelectedServer] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);

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
    setSelectedServer(server);
    setError(null);
  };

  return (
    <div className="App">
      <div className="container">
        <header className="header">
          <h1>Speed Test</h1>
          <p>Measure your internet connection speed</p>
        </header>

        {error && (
          <div className="error">
            {error}
          </div>
        )}

        <ServerSelect
          servers={servers}
          selectedServer={selectedServer}
          onServerSelect={handleServerSelect}
          loading={loading}
        />

        {selectedServer && (
          <SpeedTest
            server={selectedServer}
            onError={setError}
          />
        )}
      </div>
    </div>
  );
}

export default App; 