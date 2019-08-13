#!/bin/bash
set -e

cargo build

echo "Starting server"
../../target/debug/server &
PID=$!
sleep 2

echo "Testing server with client"
../../target/debug/client

kill $PID
