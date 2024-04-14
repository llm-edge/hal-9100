
Before starting, make sure you have a modern laptop or desktop with at least 16GB of RAM and a modern CPU. You will also need to have Node.js installed.

This LLM runs at 60+ tokens per second on my MacBook Pro M2.

At the moment, you need **Docker** installed to run the API.

We will use [OpenAI's JS SDK](https://github.com/openai/openai-node), but feel free to use the [python one](https://github.com/openai/openai-python), you can copy paste this doc in chatgpt to translate to python!

# Action tool

Action allows you to describe an API to the Assistants and have it intelligently do requests autonomously to the API. This is a powerful feature that allows you to build Assistants that can interact with the world in a meaningful way.

We'll use `gemma` which is one of the best 7b sized open source LLM to this date, thanks to Mistral!

## Steps to Run the API

1. **Run Mistral open source LLM**

We'll be using [Ollama](https://github.com/ollama/ollama) to run the LLM, but many options are available, [let me know if you need help or want to run this in your infra](mailto:hi@louis030195.com).

Please follow Ollama instructions for installation, then run:

```bash
ollama run gemma:7b
```

Open a second terminal, and run the following command to run the API:

```bash
docker compose --profile api -f docker/docker-compose.yml up
```

Open a second terminal, and run the following command to test the API:

```bash
# Test if it works properly:
# Install the OpenAI API JS client
npm i openai 

# Start a node repl
node
```

```ts
// Paste the following code in your node repl and press enter:
const OpenAI = require('openai');

const openai = new OpenAI({
    baseURL: 'http://localhost:3000',
    apiKey: 'EMPTY',
});

async function main() {
    const stream = await openai.chat.completions.create({
        model: 'gemma:7b',
        messages: [
            {
                "role": "user",
                "content": "What is the weather like in Boston?"
            }
        ],
        tools: [
            {
                "type": "function",
                "function": {
                    "name": "get_current_weather",
                    "description": "Get the current weather in a given location (usually in user's message)",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "The city and state, e.g. San Francisco, CA"
                            },
                            "unit": {
                                "type": "string",
                                "enum": ["celsius", "fahrenheit"]
                            }
                        },
                        "required": ["location"]
                    }
                }
            }
        ],
        tool_choice: "auto",
        stream: true
    })
  
  
  for await (const chatCompletion of stream) {
    console.log(JSON.stringify(chatCompletion, null, 2));
  }
}

main();
```

You should see something like this:
```jso
{
  "content": null,
  "tool_calls": [
    {
      "id": "0b9f9b87-71dd-4746-8df6-ee76dbda2fa7",
      "type": "function",
      "function": {
        "name": "get_current_weather",
        "arguments": "{\"location\":\"Boston\",\"unit\":\"fahrenheit\"}"
      }
    }
  ],
  "role": "assistant",
  "function_call": null
}
```

Great! This also works with the Assistants endpoint of course!
