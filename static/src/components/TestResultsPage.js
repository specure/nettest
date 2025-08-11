import React, { useState, useEffect } from 'react';
import { Link } from 'react-router-dom';
import './TestResultsPage.css';

const TestResultsPage = () => {
  console.log('TestResultsPage component rendered'); // Отладочная информация
  const [results, setResults] = useState([]);
  const [loading, setLoading] = useState(false);


  const HARDCODED_UUID = 'ee7760ec-db94-43df-b8dc-001384f0ed39';

  useEffect(() => {
    console.log('TestResultsPage useEffect triggered'); // Отладочная информация
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
          const formattedResults = data.content.map(item => {
            console.log('Raw item:', item); // Отладочная информация
            
            const formattedItem = {
              id: item.openTestUuid,
              timestamp: item.measurementDate,
              ping: item.ping ? (item.ping / 1000000) : 0, // Конвертируем наносекунды в миллисекунды
              download: item.download ? (item.download / 100) : 0, // Конвертируем сотые доли Mbps в Mbps
              upload: item.upload ? (item.upload / 100) : 0, // Конвертируем сотые доли Mbps в Mbps
              openTestUuid: item.openTestUuid || 'N/A'
            };
            
            console.log('Formatted item:', formattedItem); // Отладочная информация
            return formattedItem;
          });
          
          setResults(formattedResults);
          
          // Загружаем Git историю для каждого результата
          await loadGitHistory(formattedResults);
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

  const loadGitHistory = async (results) => {
    try {
      // Получаем Git историю из GitHub API
      const response = await fetch('https://api.github.com/repos/specure/nettest/commits?per_page=100');
      
      if (!response.ok) {
        console.error('Failed to fetch Git history:', response.status);
        return;
      }
      
      const commits = await response.json();
      const commitMap = new Map();
      
      // Создаем карту commit hash -> commit message
      commits.forEach(commit => {
        commitMap.set(commit.sha, commit.commit.message);
      });
      
      // Обновляем результаты с commit message
      const updatedResults = results.map(result => {
        if (result.openTestUuid && result.openTestUuid !== 'N/A') {
          const commitMessage = commitMap.get(result.openTestUuid);
          if (commitMessage) {
            return { ...result, commitMessage };
          } else {
            return { ...result, commitMessage: 'Commit not found' };
          }
        } else {
          return { ...result, commitMessage: 'N/A' };
        }
      });
      
      setResults(updatedResults);
    } catch (error) {
      console.error('Error loading Git history:', error);
      // В случае ошибки, добавляем заглушку для commit message
      const updatedResults = results.map(result => ({
        ...result,
        commitMessage: 'Git history unavailable'
      }));
      setResults(updatedResults);
    }
  };



  const formatDate = (dateString) => {
    if (!dateString) return 'N/A';
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
                    <th>Commit Message</th>
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
                      <td className="commit-message">
                        {result.commitMessage || 'Loading...'}
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
