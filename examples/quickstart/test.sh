#!/bin/bash
set -e

cargo build

echo "Starting server"
killall quickstart -q && true
../../target/debug/quickstart &
sleep 0.5

echo "Test name endpoint"
curl -s --fail http://localhost:8000/Bobert | grep "Bobert"

# check we can see the api
curl -s --fail http://localhost:8000/ui.html > /dev/null
curl -s --fail http://localhost:8000/spec.json > /dev/null

echo
echo "Tests passed"

killall quickstart
