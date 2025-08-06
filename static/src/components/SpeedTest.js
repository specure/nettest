import React, { useState, useRef } from 'react';
import './SpeedTest.css';
import PingDisplay from './PingDisplay';
import SpeedChart from './SpeedChart';
import RMBTClient from '../utils/RMBTClient';

const SpeedTest = ({ server, onError }) => {
  const [testState, setTestState] = useState('idle'); // idle, connecting, testing, completed, error
  const [progress, setProgress] = useState(0);
  const [status, setStatus] = useState('');
  const [downloadData, setDownloadData] = useState([]);
  const [uploadData, setUploadData] = useState([]);
  const [pingMedian, setPingMedian] = useState(null);
  const [currentPhase, setCurrentPhase] = useState('INIT');
  const [results, setResults] = useState({
    download: null,
    upload: null,
    ping: null,
    jitter: null
  });

  const clientRef = useRef(null);

  const getPhaseDescription = (phase) => {
    switch (phase) {
      case 'INIT':
        return 'Initializing test...';
      case 'INIT_DOWN':
        return 'Preparing download test...';
      case 'PING':
        return 'Measuring ping...';
      case 'DOWN':
        return 'Testing download speed...';
      case 'INIT_UP':
        return 'Preparing upload test...';
      case 'UP':
        return 'Testing upload speed...';
      case 'END':
        return 'Test completed!';
      default:
        return 'Testing...';
    }
  };

  const startTest = async () => {
    try {
      setTestState('connecting');
      setProgress(10);
      setStatus('Initializing test...');
      setDownloadData([]); // Сбрасываем данные download
      setUploadData([]); // Сбрасываем данные upload
      setPingMedian(null); // Сбрасываем ping median
      setCurrentPhase('INIT');

      // Create RMBT client with rmbtws and server ID
      clientRef.current = new RMBTClient(server.webAddress, server.id);
      
      // Set up ping median callback
      clientRef.current.setOnPingMedianUpdate((median) => {
        console.log('=== SpeedTest: Ping median received ===');
        console.log('Ping median:', median);
        console.log('Median type:', typeof median);
        console.log('Setting pingMedian state to:', median);
        setPingMedian(median);
      });

      // Set up download chart callback
      clientRef.current.setOnDownloadUpdate((data) => {
        console.log('=== SpeedTest: Download data received ===');
        console.log('Download data:', data);
        console.log('Data length:', data.length);
        setDownloadData(data);
      });

      // Set up upload chart callback
      clientRef.current.setOnUploadUpdate((data) => {
        console.log('=== SpeedTest: Upload data received ===');
        console.log('Upload data:', data);
        console.log('Data length:', data.length);
        setUploadData(data);
      });

      // Set up phase update callback
      clientRef.current.setOnPhaseUpdate((phase, result) => {
        console.log('=== SpeedTest: Phase update ===');
        console.log('Phase:', phase);
        console.log('Result:', result);
        setCurrentPhase(phase);
        setStatus(getPhaseDescription(phase));
        
        // Обновляем прогресс на основе фазы
        switch (phase) {
          case 'INIT':
            setProgress(10);
            break;
          case 'INIT_DOWN':
            setProgress(20);
            break;
          case 'PING':
            setProgress(30);
            break;
          case 'DOWN':
            setProgress(50);
            break;
          case 'INIT_UP':
            setProgress(70);
            break;
          case 'UP':
            setProgress(80);
            break;
          case 'END':
            setProgress(100);
            break;
          default:
            break;
        }
      });
      
      // Connect to server with timeout
      const connectPromise = clientRef.current.connect();
      const timeoutPromise = new Promise((_, reject) => 
        setTimeout(() => reject(new Error('Connection timeout')), 15000)
      );
      
      await Promise.race([connectPromise, timeoutPromise]);
      
      setProgress(20);
      setStatus('Connected to server');

      // Run full test using rmbtws
      setProgress(30);
      setStatus('Running speed test...');
      
      const testResults = await clientRef.current.runFullTest();
      
      // Сохраняем пинг из результатов теста
      const finalPing = testResults.ping;
      if (finalPing !== null) {
        setPingMedian(finalPing);
      }
      
      setResults({
        download: testResults.download,
        upload: testResults.upload,
        ping: testResults.ping,
        jitter: null // Убираем джиттер
      });
      
      setProgress(100);
      setStatus('Test completed!');
      setTestState('completed');

    } catch (error) {
      console.error('Test error:', error);
      setStatus(`Test failed: ${error.message}`);
      setTestState('error');
      onError(`Test failed: ${error.message}`);
    } finally {
      if (clientRef.current) {
        clientRef.current.disconnect();
      }
    }
  };

  const resetTest = () => {
    setTestState('idle');
    setProgress(0);
    setStatus('');
    setDownloadData([]);
    setUploadData([]);
    setPingMedian(null);
    setCurrentPhase('INIT');
    setResults({
      download: null,
      upload: null,
      ping: null,
      jitter: null
    });
  };

  return (
    <div className="speed-test">
      <button 
        className={`test-button ${testState === 'testing' || testState === 'connecting' ? 'disabled' : ''}`}
        onClick={startTest}
        disabled={testState === 'testing' || testState === 'connecting'}
      >
        {testState === 'testing' || testState === 'connecting' ? 'Testing...' : 'Start Test'}
      </button>

      {(testState === 'connecting' || testState === 'testing') && (
        <div className="progress">
          <div className="progress-bar">
            <div 
              className="progress-fill" 
              style={{ width: `${progress}%` }}
            ></div>
          </div>
          <div className="status">{status}</div>
          <div className="phase-info">
            Current Phase: <strong>{currentPhase}</strong>
          </div>
        </div>
      )}

      {/* Все три компонента отображаются всегда */}
      <div className="charts-container">
        <PingDisplay 
          pingMedian={pingMedian}
          isActive={testState === 'testing' || testState === 'connecting' || (testState === 'completed' && pingMedian !== null)} 
        />
        
        <SpeedChart 
          data={downloadData}
          title="Download Speed"
          isActive={testState === 'testing' || testState === 'connecting'}
          color="#ff6b6b"
        />
        
        <SpeedChart 
          data={uploadData}
          title="Upload Speed"
          isActive={testState === 'testing' || testState === 'connecting'}
          color="#4ecdc4"
        />
      </div>

      {testState === 'completed' && (
        <div className="results">
          <div className="result-item">
            <span className="result-label">Download Speed</span>
            <span className="result-value">
              {results.download ? `${results.download.toFixed(1)} Mbps` : '-'}
            </span>
          </div>
          <div className="result-item">
            <span className="result-label">Upload Speed</span>
            <span className="result-value">
              {results.upload ? `${results.upload.toFixed(1)} Mbps` : '-'}
            </span>
          </div>
          <div className="result-item">
            <span className="result-label">Ping</span>
            <span className="result-value">
              {results.ping ? `${results.ping.toFixed(1)} ms` : '-'}
            </span>
          </div>
          <button className="reset-button" onClick={resetTest}>
            Run Test Again
          </button>
        </div>
      )}

      {testState === 'error' && (
        <div className="error-message">
          <p>{status}</p>
          <button className="reset-button" onClick={resetTest}>
            Try Again
          </button>
        </div>
      )}
    </div>
  );
};

export default SpeedTest; 