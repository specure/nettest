import React from 'react';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import './PingChart.css';

const PingChart = ({ pingData, isActive }) => {
  console.log('=== PingChart render ===');
  console.log('pingData:', pingData);
  console.log('isActive:', isActive);
  console.log('pingData length:', pingData?.length);
  console.log('pingData type:', typeof pingData);

  if (!isActive || !pingData || pingData.length === 0) {
    console.log('PingChart: showing placeholder');
    return (
      <div className="ping-chart-container">
        <h3>Ping Chart</h3>
        <div className="ping-chart-placeholder">
          {isActive ? 'Waiting for ping data...' : 'Chart will appear when test starts'}
        </div>
      </div>
    );
  }

  console.log('PingChart: rendering chart with data:', pingData);
  return (
    <div className="ping-chart-container">
      <h3>Ping Chart</h3>
      <ResponsiveContainer width="100%" height={200}>
        <LineChart data={pingData}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis 
            dataKey="time" 
            type="number"
            domain={['dataMin', 'dataMax']}
            tickFormatter={(value) => `${Math.round(value)}s`}
          />
          <YAxis 
            domain={[0, 'dataMax + 10']}
            tickFormatter={(value) => `${value}ms`}
          />
          <Tooltip 
            formatter={(value, name) => [`${value}ms`, 'Ping']}
            labelFormatter={(value) => `Time: ${Math.round(value)}s`}
          />
          <Line 
            type="monotone" 
            dataKey="ping" 
            stroke="#8884d8" 
            strokeWidth={2}
            dot={{ fill: '#8884d8', strokeWidth: 2, r: 4 }}
            activeDot={{ r: 6, stroke: '#8884d8', strokeWidth: 2 }}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
};

export default PingChart; 