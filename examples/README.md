# Open Source Assistants API Quickstart Guide

This guide demonstrates how to use the Open Source Assistants API to create an assistant that can answer questions about the weather using function calling.

## Prerequisites

We will use Perplexity API to get started quickly with an LLM but you can run this example with any LLM.

1. Get an API key from Perplexity. You can get it [here](https://docs.perplexity.ai/docs). 
2. Install OpenAI SDK: `npm i openai`

## Setup

1. Start Postgres, Redis, and Minio: `make reboot`. 
2. In another terminal, start the API server: `make all`

## Running the Script

Run the script using Node.js: `node ./examples/quickstart.js`

## Troubleshooting

If you run into any issues, here's what you can do:
- Restart the infrastructure: `make reboot`
- Restart the API server: `make all`

If you still run into issues, please contact @louis030195 on [Discord](https://discord.gg/XMetBW3zCG).
Or book a call [here](https://cal.com/louis030195/unleash-llms). 
