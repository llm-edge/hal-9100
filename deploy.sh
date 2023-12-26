#!/bin/bash

set -e
set -u

source .env

kubectl create namespace assistants 

kubectl delete secret anthropic-api-key -n assistants || true
kubectl create secret generic anthropic-api-key --from-literal=ANTHROPIC_API_KEY=$ANTHROPIC_API_KEY -n assistants

kubectl delete secret model-url -n assistants || true
kubectl create secret generic model-url --from-literal=MODEL_URL=$MODEL_URL -n assistants

kubectl delete secret model-api-key -n assistants || true
kubectl create secret generic model-api-key --from-literal=MODEL_API_KEY=$MODEL_API_KEY -n assistants

kubectl create configmap migration-script --from-file=assistants-core/src/migrations.sql -n assistants

kubectl apply -f ee/k8s/one-liner-everything.yaml -n assistants
