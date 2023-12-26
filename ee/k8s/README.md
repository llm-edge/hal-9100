# Deploying Assistants to Kubernetes

The simplest way to deploy Assistants is on Kubernetes using the provided `deploy.sh` script.

Before running the script, make sure you have a `.env` file in your project root with the following variables set:

- `ANTHROPIC_API_KEY`: Your Anthropic API key.
- `MODEL_URL`: The URL of your model.
- `MODEL_API_KEY`: The API key for your model.

Once you have your `.env` file set up, you can deploy Assistants to Kubernetes with the following command:

```bash
./deploy.sh
# The `deploy.sh` script will handle all the necessary Kubernetes configurations for you.
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

