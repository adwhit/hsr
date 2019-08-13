#!/bin/bash
set -e

cargo build

echo "Starting server"
killall petstore-server -q && true
../../target/debug/petstore-server &
sleep 0.5

echo "Testing server with client"
../../target/debug/petstore-client

# check we can see the api
curl -s --fail http://localhost:8000/ui.html > /dev/null
curl -s --fail http://localhost:8000/spec.json > /dev/null

echo "Tests passed"

killall petstore-server
