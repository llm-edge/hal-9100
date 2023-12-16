# Open Source Assistants API Quickstart Guide

This guide demonstrates how to use the Open Source Assistants API to create an assistant that can answer questions about the weather using function calling.

## Prerequisites

We will use Perplexity API to get started quickly with an LLM but you can run this example with any LLM.

1. Get an API key from Perplexity. You can get it [here](https://docs.perplexity.ai/docs). 
2. Install OpenAI SDK: `npm i openai`

## Setup

1. Start Postgres, Redis, Minio, and the server: `make reboot && make all`. This will take a few seconds.

## Running the Script

Run the script using Node.js: `node ./examples/quickstart.js`

## What did happen?

In `quickstart.js`, we're creating a weather assistant using the Open Source Assistants API. Here's a step-by-step breakdown:

1. **Setup**: We import the OpenAI SDK and initialize it with our API key and base URL.

2. **Create Assistant**: We create an assistant with specific instructions and tools. In this case, the assistant is a weather bot that uses a function to get the current weather.

3. **Create Thread**: We create a new thread for the assistant to operate in.

4. **Create Message**: We create a user message asking about the weather in San Francisco.

5. **Create Run**: We create a run, which is an instance of the assistant performing its task.

6. **Get Run**: We retrieve the run to check its status. If the run requires action (like fetching the weather), we handle that.

7. **Submit Tool Outputs**: If the run required action, we fetch the weather and submit the output back to the run.

8. **Get Messages**: Finally, we retrieve all messages in the thread. This includes the user's original question and the assistant's response.

This script demonstrates how to use the Open Source Assistants API to create an interactive assistant that can answer questions using function calls.

## What's Next?

Now that you've got your feet wet with the Open Source Assistants API, it's time to dive deeper. Check out the `examples` directory for more complex examples and use-cases. 

For those interested in self-hosting, take a look at the [Self-Hosting Guide](./ee/k8s/README.md) in the `./ee/k8s/` directory. It provides detailed instructions on how to set up and manage your own instance.

If you're looking for inspiration on what to build next, check out the [IDEAS.md](./examples/IDEAS.md) file in the `examples` directory. It contains a list of project ideas that leverage the power of the Open Source Assistants API, ranging from AI-powered personal budgeting apps to language learning apps, health trackers, and more.

You can also explore the OpenAI Examples for a wider range of applications and to understand how to leverage the full power of the API.

Remember, the only limit is your imagination. Happy coding!

## Troubleshooting

If you run into any issues, here's what you can do:
- Restart the infrastructure: `make reboot`
- Restart the API server: `make all`

If you still run into issues, please contact @louis030195 on [Discord](https://discord.gg/XMetBW3zCG).
Or book a call [here](https://cal.com/louis030195/unleash-llms). 
