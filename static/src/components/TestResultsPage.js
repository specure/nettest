import React, { useState, useEffect } from 'react';
import { Link } from 'react-router-dom';
import './TestResultsPage.css';

const TestResultsPage = () => {
  const [results, setResults] = useState([]);
  const [loading, setLoading] = useState(false);
  const [stats, setStats] = useState({
    latestDownload: 0,
    latestUpload: 0,
    latestPing: 0
  });

  const HARDCODED_UUID = 'ee7760ec-db94-43df-b8dc-001384f0ed38';

  useEffect(() => {
    loadResults();
  }, []);

  const loadResults = async () => {
    try {
      setLoading(true);
      
      // Получаем историю измерений из API по захардкоженному UUID
      const response = await fetch(`https://api.nettest.org/reports/basic/history?page=1&size=50&sort=measurementDate,desc&uuid=${HARDCODED_UUID}`, {
        headers: {
          'x-nettest-client': 'nt',
          'Content-Type': 'application/json'
        }
      });
      
      if (response.ok) {
        const data = await response.json();
        if (data && data.content && Array.isArray(data.content)) {
          const formattedResults = data.content.map(item => ({
            id: item.openTestUuid,
            timestamp: item.measurementDate,
            ping: item.pingMedian || 0,
            download: item.speedDownload ? (item.speedDownload / 1000000) : 0, // Конвертируем в Mbps
            upload: item.speedUpload ? (item.speedUpload / 1000000) : 0, // Конвертируем в Mbps
            openTestUuid: item.openTestUuid || 'N/A'
          }));
          
          setResults(formattedResults);
          updateStats(formattedResults);
        } else {
          setResults([]);
        }
      } else {
        console.log('No results found for UUID:', HARDCODED_UUID);
        setResults([]);
      }
    } catch (error) {
      console.error('Error loading results:', error);
      setResults([]);
    } finally {
      setLoading(false);
    }
  };

  const updateStats = (results) => {
    if (results.length === 0) return;
    
    // Берем первый (самый последний) результат для latest значений
    const latestResult = results[0];
    
    setStats({
      latestDownload: latestResult.download,
      latestUpload: latestResult.upload,
      latestPing: latestResult.ping
    });
  };

  const formatDate = (dateString) => {
    const date = new Date(dateString);
    return date.toLocaleString();
  };

  const formatSpeed = (speed) => {
    return `${speed.toFixed(2)} Mbps`;
  };

  const formatPing = (ping) => {
    return `${ping.toFixed(2)} ms`;
  };

  return (
    <div className="test-results-page">
      <div className="page-header">
        <div className="container">
          <Link to="/" className="back-link">← Back to Nettest</Link>
          <h1>🚀 Test Results History</h1>
          <p>UUID: {HARDCODED_UUID}</p>
        </div>
      </div>
      
      <div className="container">
        {/* Statistics Cards */}
        <div className="stats-grid">
          <div className="stat-card">
            <h3>⚡ Latest Download</h3>
            <div className="stat-value">{formatSpeed(stats.latestDownload)}</div>
            <div className="stat-trend">Most recent test</div>
          </div>
          <div className="stat-card">
            <h3>📈 Latest Upload</h3>
            <div className="stat-value">{formatSpeed(stats.latestUpload)}</div>
            <div className="stat-trend">Most recent test</div>
          </div>
          <div className="stat-card">
            <h3>🎯 Latest Ping</h3>
            <div className="stat-value">{formatPing(stats.latestPing)}</div>
            <div className="stat-trend">Most recent test</div>
          </div>
        </div>
        
        {/* Results Table */}
        <div className="results-table">
          <h3>📋 Test Results History</h3>
          <div className="results-content">
            {loading ? (
              <div className="loading">Loading test results...</div>
            ) : results.length === 0 ? (
              <div className="empty-state">
                <p>No test results available</p>
                <p>Run tests with -save to see results here</p>
              </div>
            ) : (
              <table className="results-table-content">
                <thead>
                  <tr>
                    <th>Date</th>
                    <th>Test ID</th>
                    <th>Ping (ms)</th>
                    <th>Download (Mbps)</th>
                    <th>Upload (Mbps)</th>
                  </tr>
                </thead>
                <tbody>
                  {results.map((result) => (
                    <tr key={result.id} className="result-row">
                      <td>{formatDate(result.timestamp)}</td>
                      <td>
                        <span className="test-id">{result.openTestUuid.substring(0, 8)}</span>
                      </td>
                      <td>{formatPing(result.ping)}</td>
                      <td>{formatSpeed(result.download)}</td>
                      <td>{formatSpeed(result.upload)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>
        
        <button className="refresh-btn" onClick={loadResults}>
          🔄 Refresh Results
        </button>
      </div>
    </div>
  );
};

export default TestResultsPage;
