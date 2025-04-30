#!/bin/bash

# Configuration
TEST_INTERVAL=1  # seconds between adding new threads
INITIAL_THREADS=5  # starting number of parallel threads
MAX_THREADS=200  # maximum number of parallel threads

# Function to run tests with specified number of threads
run_tests() {
    local threads=$1
    echo "Starting tests with $threads parallel threads..."
    TEST_USE_DEFAULT_PORTS=1 cargo test -- --test-threads=$threads > /dev/null 2>&1 &
    echo "Started test process with $threads threads"
}

# Main loop
current_threads=$INITIAL_THREADS

echo "Starting test runs with increasing number of threads"
echo "Initial threads: $INITIAL_THREADS"
echo "Maximum threads: $MAX_THREADS"
echo "Adding new threads every $TEST_INTERVAL seconds"

# Start initial tests
run_tests $current_threads

while [ $current_threads -lt $MAX_THREADS ]; do
    echo "----------------------------------------"
    echo "Current threads: $current_threads"
    
    # Wait for interval
    sleep $TEST_INTERVAL
    
    # Increase thread count
    current_threads=$((current_threads + 5))
    
    # Start new tests
    run_tests $current_threads
done

echo "----------------------------------------"
echo "All test processes started"
echo "Press Ctrl+C to stop all tests" 