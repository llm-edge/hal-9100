# Deploying Assistants to Kubernetes

If you want to run this, please DM @louis030195 (Discord, Twitter, etc.). He will help you instantly.

At the moment the simplest way to deploy Assistants is on Kubernetes.

First, you need to deploy an LLM. 
For more detailed instructions on deploying a Mistral LLM, check out their documentation: [Mistral Deployment Documentation](https://docs.mistral.ai/self-deployment/overview).
This should give you a comprehensive guide on setting up and managing your deployment.

Then you can deploy Assistants to Kubernetes:

```bash
# Create a new namespace for your assistants
kubectl create namespace assistants 

# Create a secret for the model URL, extracting it from your .env file
kubectl create secret generic model-url --from-literal=MODEL_URL=$(grep MODEL_URL .env | head -n 1 | cut -d '=' -f2) -n assistants

# If your LLM requires an API key, create a secret for it, again extracting from your .env file
kubectl create secret generic model-api-key --from-literal=MODEL_API_KEY=$(grep MODEL_API_KEY .env | head -n 1 | cut -d '=' -f2) -n assistants

# Create a ConfigMap for the migration script
kubectl create configmap migration-script --from-file=assistants-core/src/migrations.sql -n assistants

# Apply the Kubernetes configurations defined in your YAML file
make kubernetes-deploy-assistants 
```

## Useful debugging commands

```bash
# Get the status of all pods in the 'assistants' namespace
kubectl get pods -n assistants -l app=assistants

# Store the name of the first pod in the 'assistants' app into a variable
POD_NAME=$(kubectl get pods -n assistants -l app=assistants -o jsonpath="{.items[0].metadata.name}")

# View logs of the specified pod, useful for troubleshooting
kubectl logs $POD_NAME -n assistants -c rust-api

# Retrieve the IP address of the rust-api-service and store it in a variable
URL=$(kubectl get svc rust-api-service -n assistants -o jsonpath="{.status.loadBalancer.ingress[0].ip}")

# Test the connection to your service with a curl command
curl -X GET http://$URL/threads/1/runs/1 -H "Content-Type: application/json"
```

If you need special support, [please reach out](https://cal.com/louis030195/unleash-llms).

