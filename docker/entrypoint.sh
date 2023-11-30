#!/bin/bash

# Run the consumer and server applications concurrently
./run_consumer &
./assistants-api-communication &

# Wait for all background processes to finish
wait