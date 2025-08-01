#!/bin/bash

echo "🌐 Starting local server for Measurement Servers Map..."
echo "📍 Map will be available at: http://localhost:8000/servers_map.html"
echo "🔄 Press Ctrl+C to stop the server"
echo ""

# Check if Python 3 is available
if command -v python3 &> /dev/null; then
    python3 -m http.server 8000
elif command -v python &> /dev/null; then
    python -m SimpleHTTPServer 8000
else
    echo "❌ Error: Python is not installed. Please install Python to serve the map locally."
    exit 1
fi 