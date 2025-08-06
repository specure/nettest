import React, { useEffect, useRef } from 'react';
import './ServersMap.css';

const ServersMap = ({ servers, onServerSelect }) => {
  const mapRef = useRef(null);
  const mapInstanceRef = useRef(null);
  const markersRef = useRef([]);

  useEffect(() => {
    // Load Leaflet CSS dynamically
    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = 'https://unpkg.com/leaflet@1.9.4/dist/leaflet.css';
    document.head.appendChild(link);

    // Load Leaflet JS dynamically
    const script = document.createElement('script');
    script.src = 'https://unpkg.com/leaflet@1.9.4/dist/leaflet.js';
    script.onload = () => {
      initializeMap();
    };
    document.head.appendChild(script);

    return () => {
      // Cleanup
      if (mapInstanceRef.current) {
        mapInstanceRef.current.remove();
      }
    };
  }, []);

  const initializeMap = () => {
    if (!window.L || mapInstanceRef.current) return;

    try {
      // Initialize map
      const map = window.L.map(mapRef.current).setView([0, 0], 2);
      mapInstanceRef.current = map;

      // Using a dark theme tile layer from CartoDB
      window.L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', {
        attribution: '© OpenStreetMap contributors, © CARTO'
      }).addTo(map);

      // Wait for map to be ready
      map.whenReady(() => {
        console.log('Map is ready, updating markers');
        updateMapMarkers();
      });

      console.log('Map initialized successfully');
    } catch (error) {
      console.error('Error initializing map:', error);
    }
  };

  const updateMapMarkers = () => {
    if (!mapInstanceRef.current || !servers) {
      console.log('Map not ready or no servers');
      return;
    }

    // Check if map is ready
    if (!mapInstanceRef.current._loaded) {
      console.log('Map not loaded yet, waiting...');
      setTimeout(updateMapMarkers, 100);
      return;
    }

    // Clear existing markers
    markersRef.current.forEach(marker => {
      try {
        mapInstanceRef.current.removeLayer(marker);
      } catch (error) {
        console.warn('Error removing marker:', error);
      }
    });
    markersRef.current = [];

    // Filter servers with valid coordinates
    const validServers = servers.filter(server => 
      server.location && 
      server.location.latitude !== 0 && 
      server.location.longitude !== 0 &&
      !isNaN(server.location.latitude) && 
      !isNaN(server.location.longitude) &&
      server.location.latitude >= -90 && 
      server.location.latitude <= 90 &&
      server.location.longitude >= -180 && 
      server.location.longitude <= 180
    );

    console.log('Total servers:', servers.length);
    console.log('Valid servers with coordinates:', validServers.length);
    console.log('Valid servers:', validServers);

    if (validServers.length === 0) {
      console.warn('No servers with valid coordinates found');
      // Set default view if no valid servers
      mapInstanceRef.current.setView([0, 0], 2);
      return;
    }

    // Add markers for each server
    validServers.forEach(server => {
      console.log('Adding marker for server:', server.name, 'with webAddress:', server.webAddress);
      
      try {
        const marker = window.L.marker([server.location.latitude, server.location.longitude])
          .addTo(mapInstanceRef.current)
          .bindPopup(`
            <div class="server-popup">
              <b>${server.name}</b><br>
              City: ${server.city}<br>
              Web Address: ${server.webAddress}<br>
              Distance: ${server.distance ? server.distance.toFixed(2) : 'N/A'} km<br>
              Version: ${server.version || 'N/A'}<br>
              IPv4: ${server.ipV4Support ? 'Yes' : 'No'}<br>
              IPv6: ${server.ipV6Support ? 'Yes' : 'No'}<br>
              Dedicated: ${server.dedicated ? 'Yes' : 'No'}
              <br><br>
              <button onclick="window.selectServer('${server.webAddress}')" class="select-server-btn">
                Start Test
              </button>
            </div>
          `);
        
        markersRef.current.push(marker);
      } catch (error) {
        console.error('Error adding marker for server:', server.name, error);
      }
    });

    // Fit map to show all markers only if we have valid markers
    if (markersRef.current.length > 0) {
      try {
        const group = window.L.featureGroup(markersRef.current);
        const bounds = group.getBounds();
        
        // Check if bounds are valid
        if (bounds.isValid()) {
          mapInstanceRef.current.fitBounds(bounds.pad(0.1));
          console.log('Map bounds set successfully');
        } else {
          console.warn('Invalid bounds, setting default view');
          mapInstanceRef.current.setView([0, 0], 2);
        }
      } catch (error) {
        console.error('Error setting map bounds:', error);
        mapInstanceRef.current.setView([0, 0], 2);
      }
    } else {
      // Set default view if no markers
      mapInstanceRef.current.setView([0, 0], 2);
    }
  };

  useEffect(() => {
    if (servers && servers.length > 0 && mapInstanceRef.current) {
      console.log('Servers data updated, updating map markers');
      console.log('Servers:', servers);
      
      // Wait a bit for map to be fully ready
      setTimeout(() => {
        updateMapMarkers();
      }, 100);
    } else {
      console.log('No servers data available or map not ready');
    }
  }, [servers]);

  // Expose selectServer function globally
  useEffect(() => {
    window.selectServer = (serverWebAddress) => {
      console.log('selectServer called with:', serverWebAddress);
      console.log('Available servers:', servers);
      
      const server = servers.find(s => s.webAddress === serverWebAddress);
      if (server && onServerSelect) {
        console.log('Server selected from map:', server);
        onServerSelect(server);
        
        // Close popup after selection
        if (mapInstanceRef.current) {
          mapInstanceRef.current.closePopup();
        }
      } else {
        console.error('Server not found:', serverWebAddress);
        console.log('Available servers:', servers);
      }
    };

    return () => {
      delete window.selectServer;
    };
  }, [servers, onServerSelect]);

  return (
    <div className="servers-map-container">
      <div ref={mapRef} className="servers-map" />
      {!window.L && (
        <div className="map-loading">
          <div className="loading-text">Loading map...</div>
        </div>
      )}
    </div>
  );
};

export default ServersMap; 