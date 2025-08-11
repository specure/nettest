import React, { useState, useEffect } from 'react';
import { BrowserRouter as Router, Routes, Route, Link, useLocation, useNavigate } from 'react-router-dom';
import './App.css';
import SpeedTest from './components/SpeedTest';
import ServerSelect from './components/ServerSelect';
import TestModal from './components/TestModal';
import ServersMap from './components/ServersMap';
import QuickActions from './components/QuickActions';
import TestResults from './components/TestResults';
import TestResultsPage from './components/TestResultsPage';

function App() {
  const [servers, setServers] = useState([]);
  const [selectedServer, setSelectedServer] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isTestResultsOpen, setIsTestResultsOpen] = useState(false);

  // Компонент для отладки маршрутизации
  const AppContent = () => {
    const location = useLocation();
    const navigate = useNavigate();
    
    console.log('Current location:', location);
    console.log('Current pathname:', location.pathname);
    console.log('Current hash:', location.hash);
    
    useEffect(() => {
      console.log('Location changed to:', location.pathname);
    }, [location]);
    
    return (
      <Routes>
        <Route path="/" element={<MainApp />} />
        <Route path="/test-results" element={<TestResultsPage />} />
        <Route path="*" element={<MainApp />} />
      </Routes>
    );
  };

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

  const MainApp = () => {
    console.log('MainApp component rendered'); // Отладочная информация
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
                <Link to="/test-results" className="test-results-btn">
                  📊 View Test Results History
                </Link>
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
  };

  console.log('App component rendering, current path:', window.location.pathname); // Отладочная информация
  
  return (
    <Router>
      <AppContent />
    </Router>
  );
}

export default App; 