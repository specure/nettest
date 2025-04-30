#!/usr/bin/env python3

import pandas as pd
import matplotlib.pyplot as plt
from datetime import datetime

# Read the CSV file and convert columns to numeric types
system_metrics = pd.read_csv('system_metrics.csv')
system_metrics['cpu_usage'] = pd.to_numeric(system_metrics['cpu_usage'], errors='coerce')
system_metrics['memory_usage'] = pd.to_numeric(system_metrics['memory_usage'], errors='coerce')
system_metrics['network_rx'] = pd.to_numeric(system_metrics['network_rx'], errors='coerce')
system_metrics['network_tx'] = pd.to_numeric(system_metrics['network_tx'], errors='coerce')
system_metrics['active_processes'] = pd.to_numeric(system_metrics['active_processes'], errors='coerce')

# Convert timestamps to datetime
system_metrics['timestamp'] = pd.to_datetime(system_metrics['timestamp'], unit='s')

# Create figure with subplots
fig, (ax1, ax2, ax3) = plt.subplots(3, 1, figsize=(15, 12))
fig.suptitle('System Performance Metrics')

# Plot 1: CPU and Memory Usage
ax1.plot(system_metrics['timestamp'], system_metrics['cpu_usage'], label='CPU Usage (%)', color='blue')
ax1.plot(system_metrics['timestamp'], system_metrics['memory_usage'], label='Memory Usage (%)', color='orange')
ax1.set_ylabel('Usage (%)')
ax1.grid(True)
ax1.legend()

# Plot 2: Network Usage
ax2.plot(system_metrics['timestamp'], system_metrics['network_rx']/1024/1024, label='Network RX (MB)')
ax2.plot(system_metrics['timestamp'], system_metrics['network_tx']/1024/1024, label='Network TX (MB)')
ax2.set_ylabel('Network Usage (MB)')
ax2.grid(True)
ax2.legend()

# Plot 3: Active Processes
ax3.plot(system_metrics['timestamp'], system_metrics['active_processes'], label='Active Processes', color='green')
ax3.set_ylabel('Number of Processes')
ax3.set_xlabel('Time')
ax3.grid(True)
ax3.legend()

# Rotate x-axis labels
for ax in [ax1, ax2, ax3]:
    plt.setp(ax.get_xticklabels(), rotation=45)

# Adjust layout
plt.tight_layout()

# Save the plot
plt.savefig('system_metrics.png')
print("Plot saved as system_metrics.png")

# Calculate and print statistics
print("\nPerformance Statistics:")
print("----------------------")
print(f"Maximum CPU Usage: {system_metrics['cpu_usage'].max():.2f}%")
print(f"Maximum Memory Usage: {system_metrics['memory_usage'].max():.2f}%")
print(f"Maximum Network RX: {system_metrics['network_rx'].max()/1024/1024:.2f} MB")
print(f"Maximum Network TX: {system_metrics['network_tx'].max()/1024/1024:.2f} MB")
print(f"Maximum Active Processes: {system_metrics['active_processes'].max()}") 