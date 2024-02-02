# Open Source Assistants API Quickstart Guide

This guide demonstrates how to use the Open Source Assistants API to create an assistant that can answer questions about the weather using function calling.

Function calling is a more precise and automatic way to provide context to an LLM than retrieval.

## Setup

```bash
git clone https://github.com/stellar-amenities/assistants
cd assistants
cp .env.example .env
```

To get started quickly, let's use Perplexity API.
Get an API key from Perplexity. You can get it [here](https://docs.perplexity.ai/docs). Replace in [.env](./.env) the `MODEL_API_KEY` with your API key

Install OpenAI SDK: `npm i openai`

Start the infra:

```bash
docker-compose --profile api -f docker/docker-compose.yml up -d
```

Run the [quickstart](./examples/quickstart.js):

```bash
node examples/quickstart.js
```

>The current temperature in San Francisco is 68 degrees Fahrenheit.

## What did happen?

In `quickstart.js`, we're creating a weather assistant using the Open Source Assistants API. Here's a step-by-step breakdown:

1. **Setup**: We import the OpenAI SDK and initialize it with the local server as base URL.

2. **Create Assistant**: We create an assistant with specific instructions and tools. In this case, the assistant is a weather bot that uses a function to get the current weather.

3. **Create Thread**: We create a new thread for the assistant to operate in.

4. **Create Message**: We create a user message asking about the weather in San Francisco.

5. **Create Run**: We create a run, which is an instance of the assistant performing its task.

6. **Get Run**: We retrieve the run to check its status. The run will require an action through function calling. We run our function given the input provided by the assistant.

7. **Submit Tool Outputs**: Once we fetched the weather, we submit the output to the assistant.

8. **Get Messages**: Finally, we retrieve all messages in the thread. This includes the user's original question and the assistant's response. The LLM is able to answer the question by using the precise context provided by the function call.

This script demonstrates how to use the Open Source Assistants API to create an interactive assistant that can answer questions using function calls.

## What's Next?

Now that you've got your feet wet with the Open Source Assistants API, it's time to dive deeper. Check out the `examples` directory for more complex examples and use-cases. 

For those interested in self-hosting, take a look at the **Self-Hosting Guide** in the `./ee/k8s/` directory. It provides detailed instructions on how to set up and manage your own instance.

If you're looking for inspiration on what to build next, check out the `IDEAS.md`` file in the `examples` directory. It contains a list of project ideas that leverage the power of the Open Source Assistants API, ranging from AI-powered personal budgeting apps to language learning apps, health trackers, and more.

You can also explore the OpenAI Examples for a wider range of applications and to understand how to leverage the full power of the API.

Remember, the only limit is your imagination. Happy coding!

## Troubleshooting

If you run into issues, please contact @louis030195 on [Discord](https://discord.gg/XMetBW3zCG).
Or book a call [here](https://cal.com/louis030195/ai). 
