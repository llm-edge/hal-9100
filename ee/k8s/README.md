# Deploying HAL-9100 to Kubernetes

If you want to run this, please DM @louis030195 (Discord, Twitter, etc.). He will help you instantly.

At the moment the simplest way to deploy HAL-9100 is on Kubernetes.

First, you need to deploy an LLM. 
For more detailed instructions on deploying a Mistral LLM, check out their documentation: [Mistral Deployment Documentation](https://docs.mistral.ai/self-deployment/overview).
This should give you a comprehensive guide on setting up and managing your deployment.

Then you can deploy HAL-9100 to Kubernetes:

```bash
# Create a new namespace for HAL-9100
kubectl create namespace hal-9100 

# Create a secret for the model URL, extracting it from your hal-9100.toml file
kubectl create secret generic model-url --from-literal=MODEL_URL=$(grep model_url hal-9100.toml | head -n 1 | cut -d '=' -f2) -n hal-9100

# If your LLM requires an API key, create a secret for it, again extracting from your hal-9100.toml file
kubectl create secret generic model-api-key --from-literal=MODEL_API_KEY=$(grep model_api_key hal-9100.toml | head -n 1 | cut -d '=' -f2) -n hal-9100

# Create a ConfigMap for the migration script
kubectl create configmap migration-script --from-file=hal-9100-core/src/migrations.sql -n hal-9100

# Apply the Kubernetes configurations defined in your YAML file
kubectl apply -f ee/k8s/one-liner-everything.yaml -n hal-9100 
```

## Useful debugging commands

```bash
# Get the status of all pods in the 'hal-9100' namespace
kubectl get pods -n hal-9100 -l app=hal-9100

# Store the name of the first pod in the 'hal-9100' app into a variable
POD_NAME=$(kubectl get pods -n hal-9100 -l app=hal-9100 -o jsonpath="{.items[0].metadata.name}")

# View logs of the specified pod, useful for troubleshooting
kubectl logs $POD_NAME -n hal-9100 -c rust-api

# Retrieve the IP address of the rust-api-service and store it in a variable
URL=$(kubectl get svc rust-api-service -n hal-9100 -o jsonpath="{.status.loadBalancer.ingress[0].ip}")

# Test the connection to your service with a curl command
curl -X GET http://$URL/threads/1/runs/1 -H "Content-Type: application/json"
```

If you need special support, [please reach out](https://cal.com/louis030195/applied-ai).

