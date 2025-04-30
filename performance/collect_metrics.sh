#!/bin/bash

# Configuration
INTERVAL=1  # seconds between measurements
OUTPUT_FILE="system_metrics.csv"

# Create header for CSV file
echo "timestamp,cpu_usage,memory_usage,network_rx,network_tx,active_processes" > $OUTPUT_FILE

# Function to collect metrics
collect_metrics() {
    local timestamp=$(date +%s)
    
    # CPU usage (percentage)
    local cpu_usage=$(top -bn1 | grep "Cpu(s)" | awk '{print $2}')
    
    # Memory usage (percentage)
    local memory_usage=$(free | grep Mem | awk '{print $3/$2 * 100.0}')
    
    # Network usage (bytes)
    local network_rx=0
    local network_tx=0
    if [ -f /proc/net/dev ]; then
        if grep -q eth0 /proc/net/dev; then
            network_rx=$(cat /proc/net/dev | grep eth0 | awk '{print $2}')
            network_tx=$(cat /proc/net/dev | grep eth0 | awk '{print $10}')
        elif grep -q ens3 /proc/net/dev; then
            network_rx=$(cat /proc/net/dev | grep ens3 | awk '{print $2}')
            network_tx=$(cat /proc/net/dev | grep ens3 | awk '{print $10}')
        else
            network_rx=$(cat /proc/net/dev | grep -v lo | head -n1 | awk '{print $2}')
            network_tx=$(cat /proc/net/dev | grep -v lo | head -n1 | awk '{print $10}')
        fi
    fi
    
    # Number of active processes
    local active_processes=$(ps aux | wc -l)
    
    echo "$timestamp,$cpu_usage,$memory_usage,$network_rx,$network_tx,$active_processes"
}

echo "Starting metrics collection..."
echo "Press Ctrl+C to stop"

# Main loop
while true; do
    collect_metrics >> $OUTPUT_FILE
    sleep $INTERVAL
done 