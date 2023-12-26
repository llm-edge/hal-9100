#!/bin/bash

set -e
set -u

echo "Sourcing environment variables from .env file..."
source .env

echo "Building Docker image..."
docker build -t railway-assistants .

echo "Pushing Docker image to Railway registry..."
docker push railway-assistants

echo "Deploying Docker image to Railway..."
railway up

echo "Deployment to Railway completed successfully."
