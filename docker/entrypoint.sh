#!/bin/bash

# Run the executor and server applications concurrently
hal-9100-executor &
hal-9100 &

# Wait for all background processes to finish
wait