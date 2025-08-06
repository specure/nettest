import React from 'react';
import './ServerSelect.css';

const ServerSelect = ({ servers, selectedServer, onServerSelect, loading }) => {
  const handleChange = (event) => {
    const serverId = event.target.value;
    const server = servers.find(s => s.webAddress === serverId);
    onServerSelect(server || null);
  };

  if (loading) {
    return (
      <div className="server-select">
        <div className="loading">Loading servers...</div>
      </div>
    );
  }

  return (
    <div className="server-select">
      <select 
        value={selectedServer?.webAddress || ''} 
        onChange={handleChange}
        className="server-dropdown"
      >
        <option value="">Select a server...</option>
        {servers.map(server => (
          <option key={server.webAddress} value={server.webAddress}>
            {server.name} ({server.city})
          </option>
        ))}
      </select>
    </div>
  );
};

export default ServerSelect; 