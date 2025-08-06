import React, { useState, useEffect } from 'react';
import './TestModal.css';
import PingDisplay from './PingDisplay';
import SpeedChart from './SpeedChart';
import RMBTClient from '../utils/RMBTClient';

const TestModal = ({ server, isOpen, onClose }) => {
  const [testState, setTestState] = useState('idle'); // idle, connecting, testing, completed, error
  const [progress, setProgress] = useState(0);
  const [status, setStatus] = useState('');
  const [downloadData, setDownloadData] = useState([]);
  const [uploadData, setUploadData] = useState([]);
  const [pingMedian, setPingMedian] = useState(null);
  const [currentPhase, setCurrentPhase] = useState('INIT');
  const [testInfo, setTestInfo] = useState({
    serverName: '',
    remoteIp: '',
    providerName: '',
    testUUID: ''
  });
  const [results, setResults] = useState({
    download: null,
    upload: null,
    ping: null,
    jitter: null
  });
  const [error, setError] = useState(null);

  const clientRef = React.useRef(null);

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
    setTestState('connecting');
    setError(null);
    setProgress(10);
    setStatus('Initializing test...');
    setDownloadData([]);
    setUploadData([]);
    setPingMedian(null);
    setCurrentPhase('INIT');
    setTestInfo({
      serverName: '',
      remoteIp: '',
      providerName: '',
      testUUID: ''
    });

    clientRef.current = new RMBTClient(server.webAddress, server.id);

    // Set up ping median callback
    clientRef.current.setOnPingMedianUpdate((median) => {
      console.log('=== TestModal: Ping median received ===');
      console.log('Ping median:', median);
      setPingMedian(median);
    });

    // Set up download chart callback
    clientRef.current.setOnDownloadUpdate((data) => {
      console.log('=== TestModal: Download data received ===');
      console.log('Download data:', data);
      setDownloadData(data);
    });

    // Set up upload chart callback
    clientRef.current.setOnUploadUpdate((data) => {
      console.log('=== TestModal: Upload data received ===');
      console.log('Upload data:', data);
      setUploadData(data);
    });

    // Set up phase update callback
    clientRef.current.setOnPhaseUpdate((phase, result) => {
      console.log('=== TestModal: Phase update ===');
      console.log('Phase:', phase);
      console.log('Result:', result);
      setCurrentPhase(phase);
      setStatus(getPhaseDescription(phase));

      // Update progress based on phase
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

    // Set up test info callback
    clientRef.current.setOnTestInfoUpdate((info) => {
      console.log('=== TestModal: Test info received ===');
      console.log('Test info:', info);
      setTestInfo(info);
    });

    try {
      await clientRef.current.init();
      setStatus('Connecting to server...');
      const connectPromise = clientRef.current.connect();
      const timeoutPromise = new Promise((_, reject) =>
        setTimeout(() => reject(new Error('Connection timeout')), 15000)
      );

      await Promise.race([connectPromise, timeoutPromise]);

      setProgress(20);
      setStatus('Connected to server');
      setTestState('testing');

      setProgress(30);
      setStatus('Running speed test...');

      const testResults = await clientRef.current.runFullTest();

      // Save ping from test results
      const finalPing = testResults.ping;
      if (finalPing !== null) {
        setPingMedian(finalPing);
      }

      setResults({
        download: testResults.download,
        upload: testResults.upload,
        ping: testResults.ping,
        jitter: null
      });

      setProgress(100);
      setStatus('Test completed!');
      setTestState('completed');
    } catch (err) {
      console.error('Test failed:', err);
      setError(err.message);
      setStatus('Test failed!');
      setTestState('error');
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
    setTestInfo({
      serverName: '',
      remoteIp: '',
      providerName: '',
      testUUID: ''
    });
    setResults({
      download: null,
      upload: null,
      ping: null,
      jitter: null
    });
    setError(null);
  };

  const handleClose = () => {
    if (testState === 'testing' || testState === 'connecting') {
      if (clientRef.current) {
        clientRef.current.disconnect();
      }
    }
    resetTest();
    onClose();
  };

  useEffect(() => {
    if (isOpen && testState === 'idle') {
      startTest();
    }
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div className="test-modal-overlay">
      <div className="test-modal">
        <div className="test-modal-header">
          <h2>Speed Test</h2>
          <button className="close-button" onClick={handleClose}>
            ×
          </button>
        </div>

        <div className="test-modal-content">
          {/* Test Info Section */}
          {(testInfo.serverName || testInfo.remoteIp || testInfo.providerName) && (
            <div className="test-info-section">
              <h3>Test Information</h3>
              <div className="test-info-grid">
                {testInfo.serverName && (
                  <div className="test-info-item">
                    <span className="info-label">Server:</span>
                    <span className="info-value">{testInfo.serverName}</span>
                  </div>
                )}
                {testInfo.remoteIp && (
                  <div className="test-info-item">
                    <span className="info-label">IP Address:</span>
                    <span className="info-value">{testInfo.remoteIp}</span>
                  </div>
                )}
                {testInfo.providerName && (
                  <div className="test-info-item">
                    <span className="info-label">Provider:</span>
                    <span className="info-value">{testInfo.providerName}</span>
                  </div>
                )}

              </div>
            </div>
          )}

          {/* Progress Section */}
          {(testState === 'connecting' || testState === 'testing' || testState === 'completed' || testState === 'error') && (
            <div className="test-progress-container">
              <div className="progress-bar">
                <div className="progress-fill" style={{ width: `${progress}%` }}></div>
              </div>
              <div className="status">{status}</div>
            </div>
          )}

          {/* Charts Section */}
          <div className="charts-container">
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

          {/* Results Section */}
          {testState === 'completed' && (
            <div className="results">
              <h3>Test Results</h3>
              <div className="result-item">
                <span className="result-label">Download</span>
                <span className="result-value">
                  {results.download ? `${results.download.toFixed(2)} Mbps` : '-'}
                </span>
              </div>
              <div className="result-item">
                <span className="result-label">Upload</span>
                <span className="result-value">
                  {results.upload ? `${results.upload.toFixed(2)} Mbps` : '-'}
                </span>
              </div>
              <div className="result-item">
                <span className="result-label">Ping</span>
                <span className="result-value">
                  {results.ping ? `${results.ping.toFixed(1)} ms` : '-'}
                </span>
              </div>
            </div>
          )}

          {/* Error Section */}
          {testState === 'error' && (
            <div className="error-message">
              <p>Error during test:</p>
              <p>{error}</p>
              <button className="reset-button" onClick={resetTest}>
                Try Again
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default TestModal; 