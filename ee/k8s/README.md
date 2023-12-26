# Deploying Assistants to Kubernetes

To deploy Assistants to Kubernetes, simply run the `deploy.sh` script provided in this directory. The script will prompt you to enter necessary environment variables such as `ANTHROPIC_API_KEY`, `MODEL_URL`, and `MODEL_API_KEY` if they are not already set.

First, you need to deploy an LLM. 
For more detailed instructions on deploying a Mistral LLM, check out their documentation: [Mistral Deployment Documentation](https://docs.mistral.ai/self-deployment/overview).
This should give you a comprehensive guide on setting up and managing your deployment.

Then you can deploy Assistants to Kubernetes:

```bash
# Create a new namespace for your assistants
kubectl create namespace assistants 

```
./deploy.sh
``` 
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

