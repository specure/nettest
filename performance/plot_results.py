#!/usr/bin/env python3

import pandas as pd
import matplotlib.pyplot as plt
from datetime import datetime

# Read the CSV file
df = pd.read_csv('benchmark_results.csv')

# Convert timestamp to datetime
df['timestamp'] = pd.to_datetime(df['timestamp'], unit='s')

# Create figure with subplots
fig, (ax1, ax2, ax3) = plt.subplots(3, 1, figsize=(12, 10))
fig.suptitle('Server Performance Metrics Over Time')

# Plot CPU usage
ax1.plot(df['timestamp'], df['cpu_usage'], label='CPU Usage (%)')
ax1.set_ylabel('CPU Usage (%)')
ax1.grid(True)
ax1.legend()

# Plot Memory usage
ax2.plot(df['timestamp'], df['memory_usage'], label='Memory Usage (%)', color='orange')
ax2.set_ylabel('Memory Usage (%)')
ax2.grid(True)
ax2.legend()

# Plot Network usage
ax3.plot(df['timestamp'], df['network_rx']/1024/1024, label='Network RX (MB)')
ax3.plot(df['timestamp'], df['network_tx']/1024/1024, label='Network TX (MB)')
ax3.set_ylabel('Network Usage (MB)')
ax3.set_xlabel('Time')
ax3.grid(True)
ax3.legend()

# Rotate x-axis labels
plt.xticks(rotation=45)

# Adjust layout
plt.tight_layout()

# Save the plot
plt.savefig('performance_metrics.png')
print("Plot saved as performance_metrics.png") 