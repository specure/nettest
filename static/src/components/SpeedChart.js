import React from 'react';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import './SpeedChart.css';

const SpeedChart = ({ data, title, isActive, color = '#82ca9d' }) => {
  if (!data || data.length === 0) {
    return (
      <div className="speed-chart-container">
        <h3>{title}</h3>
        <div className="speed-chart-placeholder">
          {isActive ? 'Waiting for data...' : 'Chart will appear when test starts'}
        </div>
      </div>
    );
  }

  return (
    <div className="speed-chart-container">
      <h3>{title}</h3>
      <ResponsiveContainer width="100%" height={200}>
        <LineChart data={data}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis 
            dataKey="time" 
            type="number"
            domain={['dataMin', 'dataMax']}
            tickFormatter={(value) => `${Math.round(value)}s`}
          />
          <YAxis 
            domain={[0, 'dataMax + 10']}
            tickFormatter={(value) => `${value.toFixed(1)} Mbps`}
          />
          <Tooltip 
            formatter={(value, name) => [`${value.toFixed(2)} Mbps`, 'Speed']}
            labelFormatter={(value) => `Time: ${Math.round(value)}s`}
          />
          <Line 
            type="monotone" 
            dataKey="speed" 
            stroke={color} 
            strokeWidth={2}
            dot={{ fill: color, strokeWidth: 2, r: 4 }}
            activeDot={{ r: 6, stroke: color, strokeWidth: 2 }}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
};

export default SpeedChart; 