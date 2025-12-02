#!/bin/bash

echo "=== ArchDrop Performance Benchmarks ==="

# Create test files
echo "Creating test files..."
dd if=/dev/zero of=test_1gb.bin bs=1M count=1024 2>/dev/null
dd if=/dev/zero of=test_5gb.bin bs=1M count=5120 2>/dev/null

# Memory test
echo -e "\n1. Memory Usage Test (1GB file):"
archdrop send test_1gb.bin --local &
PID=$!
sleep 2
ps -p $PID -o rss,vsz,pmem,comm | tail -1
kill $PID 2>/dev/null

# Throughput test
echo -e "\n2. Throughput Test (5GB file):"
echo "Start timer when QR appears, stop when browser completes download"
archdrop send test_5gb.bin --local

# Cleanup
rm test_*.bin

echo -e "\nBenchmarks complete!"
