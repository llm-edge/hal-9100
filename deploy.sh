#!/bin/bash

# Check if the necessary environment variables are set
if [ -z "$ANTHROPIC_API_KEY" ]
then
  echo "Please enter your ANTHROPIC_API_KEY:"
  read ANTHROPIC_API_KEY
  export ANTHROPIC_API_KEY
fi

if [ -z "$MODEL_URL" ]
then
  echo "Please enter your MODEL_URL:"
  read MODEL_URL
  export MODEL_URL
fi

if [ -z "$MODEL_API_KEY" ]
then
  echo "Please enter your MODEL_API_KEY:"
  read MODEL_API_KEY
  export MODEL_API_KEY
fi

# Create the necessary Kubernetes secrets and configmaps
kubectl create namespace assistants 
kubectl create secret generic anthropic-api-key --from-literal=ANTHROPIC_API_KEY=$ANTHROPIC_API_KEY -n assistants
kubectl create secret generic model-url --from-literal=MODEL_URL=$MODEL_URL -n assistants
kubectl create secret generic model-api-key --from-literal=MODEL_API_KEY=$MODEL_API_KEY -n assistants
kubectl create configmap migration-script --from-file=assistants-core/src/migrations.sql -n assistants

# Apply the Kubernetes configurations
kubectl apply -f ee/k8s/one-liner-everything.yaml -n assistants
