#!/bin/bash

set -e
set -u

echo "Sourcing environment variables from .env file..."
source .env

echo "Building Docker image..."
docker build -t render-assistants .

echo "Pushing Docker image to Render registry..."
docker push render-assistants

echo "Deploying Docker image to Render..."
render deploy --image render-assistants

echo "Deployment to Render completed successfully."
