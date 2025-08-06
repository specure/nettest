import React, { useEffect, useState } from 'react';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import './SpeedChart.css';

const SpeedChart = React.memo(({ data, title, isActive, color = '#82ca9d' }) => {
  const [chartData, setChartData] = useState([]);
  const [updateKey, setUpdateKey] = useState(0);
}, (prevProps, nextProps) => {
  // Сравниваем только данные, игнорируем другие пропсы
  return JSON.stringify(prevProps.data) === JSON.stringify(nextProps.data);
});
  
  // Обновляем данные при изменении props
  useEffect(() => {
    console.log(`=== SpeedChart ${title} update ===`);
    console.log('New data received:', data);
    console.log('Data length:', data?.length);
    console.log('Is active:', isActive);
    
    // Обрабатываем случай пустых данных
    if (!data || data.length === 0) {
      if (chartData.length > 0) {
        console.log('Clearing chart data');
        setChartData([]);
        setUpdateKey(prev => prev + 1);
      }
      return;
    }
    
    // Простая проверка изменения данных
    if (data.length !== chartData.length || JSON.stringify(data) !== JSON.stringify(chartData)) {
      console.log('Data changed, updating chart');
      console.log('New data length:', data.length);
      console.log('Current data length:', chartData.length);
      setChartData([...data]);
      setUpdateKey(prev => prev + 1);
      console.log('Chart data updated, new key:', updateKey + 1);
    } else {
      console.log('Data unchanged, skipping update');
    }
  }, [data, chartData]); // Убираем title и isActive из зависимостей
  
  // Создаем уникальный ключ для принудительного перерендера
  const chartKey = `${title}-${updateKey}-${chartData.length}`;
  
  // Если нет данных, показываем placeholder
  if (!chartData || chartData.length === 0) {
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
    <div className={`speed-chart-container ${isActive ? 'active' : ''}`}>
      <h3>{title}</h3>
      <ResponsiveContainer width="100%" height={200}>
        <LineChart data={chartData} key={chartKey}>
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
            stroke={color === '#ff6b6b' ? '#00d4ff' : '#0099cc'}
            strokeWidth={4}
            strokeDasharray="0"
            dot={false}
            activeDot={false}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
};

export default SpeedChart; 