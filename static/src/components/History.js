import React, { useState, useEffect } from 'react';
import './History.css';
import { getClientUUID } from '../utils/uuid';

const History = ({ isOpen, onClose }) => {
  const [history, setHistory] = useState([]);
  const [loading, setLoading] = useState(false);
  const [selectedTest, setSelectedTest] = useState(null);
  const [displayedCount, setDisplayedCount] = useState(3);

  useEffect(() => {
    if (isOpen) {
      loadHistory();
      resetDisplayedCount();
    }
  }, [isOpen]);

  const loadHistory = async () => {
    try {
      setLoading(true);
      
      // Get client UUID
      const clientUUID = getClientUUID();
      
      // Get history from API with pagination and sorting parameters
      const response = await fetch(`https://api.nettest.org/reports/basic/history?page=1&size=50&sort=measurementDate,desc&uuid=${clientUUID}`, {
        headers: {
          'Content-Type': 'application/json',
          'X-Nettest-Client': 'nt'
        }
      });
      
      if (response.ok) {
        const data = await response.json();
        if (data && data.content && Array.isArray(data.content)) {
          const apiHistory = data.content.map(item => ({
            id: item.openTestUuid,
            uuid: item.openTestUuid,
            timestamp: item.measurementDate,
            server: item.measurementServerName || item.serverType || 'Unknown Server',
            download: item.download,
            upload: item.upload,
            ping: item.ping,
            status: 'completed'
          }));
          console.log('Loaded history items:', apiHistory.length);
          setHistory(apiHistory);
        } else {
          setHistory([]);
        }
      } else {
        console.log('No history found for client UUID:', clientUUID);
        setHistory([]);
      }
    } catch (error) {
      console.error('Error loading history:', error);
      setHistory([]);
    } finally {
      setLoading(false);
    }
  };

  const formatDate = (dateString) => {
    const date = new Date(dateString);
    return date.toLocaleString();
  };

  const formatSpeed = (speed) => {
    return `${speed.toFixed(1)} Mbps`;
  };

  const formatPing = (ping) => {
    return `${ping} ms`;
  };

  const handleScroll = (e) => {
    const { scrollTop, scrollHeight, clientHeight } = e.target;
    console.log('Scroll:', scrollTop, scrollHeight, clientHeight);
    if (scrollTop + clientHeight >= scrollHeight - 5) {
      // Пользователь достиг конца списка
      console.log('Reached end, increasing displayed count');
      setDisplayedCount(prev => {
        const newCount = Math.min(prev + 3, history.length);
        console.log('New displayed count:', newCount);
        return newCount;
      });
    }
  };

  const resetDisplayedCount = () => {
    setDisplayedCount(3);
  };

  if (!isOpen) return null;

  return (
    <div className="history-overlay">
      <div className="history-modal">
        <div className="history-header">
          <h2>Measurement History</h2>
          <button className="close-button" onClick={onClose}>
            ✕
          </button>
        </div>
        
        <div className="history-content" onScroll={handleScroll}>
          {loading ? (
            <div className="loading">Loading history...</div>
          ) : history.length === 0 ? (
            <div className="empty-state">
              <p>No test history available</p>
              <p>Run your first test to see results here</p>
            </div>
          ) : (
            <div className="history-list">
              {history.slice(0, displayedCount).map((test) => (
                <div 
                  key={test.id} 
                  className={`history-item ${test.status}`}
                  onClick={() => setSelectedTest(test)}
                >
                  <div className="history-item-header">
                    <span className="test-uuid">{test.uuid}</span>
                    <span className="test-date">{formatDate(test.timestamp)}</span>
                  </div>
                  <div className="history-item-details">
                    <div className="test-server">
                      <strong>Server:</strong> {test.server}
                    </div>
                    <div className="test-results">
                      <span className="result-item">
                        <strong>Download:</strong> {formatSpeed(test.download)}
                      </span>
                      <span className="result-item">
                        <strong>Upload:</strong> {formatSpeed(test.upload)}
                      </span>
                      <span className="result-item">
                        <strong>Ping:</strong> {formatPing(test.ping)}
                      </span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default History; 