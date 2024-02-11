#!/bin/bash

# Run the consumer and server applications concurrently
run_consumer &
hal-9100-api-communication &

# Wait for all background processes to finish
wait