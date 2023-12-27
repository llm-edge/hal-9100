

At the moment, you need both **Docker** installed to run the API.

Additionally, `Assistants` currently supports Anthropic and Open Source LLMs, you need some env vars that you can put in a `.env` file in the root of the project:

```bash
DATABASE_URL=postgres://postgres:secret@localhost:5432/mydatabase
REDIS_URL=redis://127.0.0.1/
S3_ENDPOINT=http://localhost:9000
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
S3_BUCKET_NAME=mybucket
MODEL_URL="http://localhost:8000/v1/chat/completions"
```

We will use [OpenAI's JS SDK](https://github.com/openai/openai-node), but feel free to use the [python one](https://github.com/openai/openai-python), you can copy paste this doc in chatgpt to translate to python!

# Function calling

Function calling allows you to describe functions to the Assistants and have it intelligently return the functions that need to be called along with their arguments. The Assistants API will pause execution during a Run when it invokes functions, and you can supply the results of the function call back to continue the Run execution.


We'll use `Intel/neural-chat-7b-v3-2` which is one of the best 7b sized open source LLM to this date, thanks to Intel!

## Steps to Run the API

1. **Run Intel open source LLM**

We'll be using [FastChat](https://github.com/lm-sys/FastChat) to run the LLM, but many options are available, [let me know if you need help or want to run this in your infra](mailto:hi@louis030195.com).

Assuming you have Python 3 and virtualenv installed.

```bash
virtualenv env
source env/bin/activate
pip3 install "fschat[model_worker]"

# Terminal 1
python3 -m fastchat.serve.controller

# Terminal 2 - FYI just change "mps" to "cpu" or "cuda" depending on your hardware
python3 -m fastchat.serve.model_worker --model-path Intel/neural-chat-7b-v3-2 --device mps --load-8bit

# Terminal 3
python3 -m fastchat.serve.openai_api_server --host localhost --port 8000

# Terminal 4
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
    baseURL: 'http://localhost:8000/v1',
    apiKey: 'EMPTY',
});

async function main() {
  const chatCompletion = await openai.chat.completions.create({
    messages: [{ role: 'user', content: 'Hello! What is your name?' }],
    model: 'neural-chat-7b-v3-2',
  });
  console.log(chatCompletion.choices[0].message.content);
}

main();
```

You should see something like this:
>Promise {
  <pending>,
  [Symbol(async_id_symbol)]: 270,
  [Symbol(trigger_async_id_symbol)]: 5
}
>
> My name is Pepper, a helpful artificial intelligence assistant.

## Function calling with Intel's LLM in JS

1. **Start the server**

```bash
docker-compose --profile api -f docker/docker-compose.yml up -d
```

2. **Create an Assistant** 

Alright, open a new terminal and start `node`, then drop these snippets step by step:

```ts
const OpenAI = require('openai');

const openai = new OpenAI({
    baseURL: 'http://localhost:3000',
    apiKey: 'EMPTY',
});
```

```ts
async function createAssistant() {
    const assistant = await openai.beta.assistants.create({
        instructions: "You are a weather bot. Use the provided functions to answer questions.",
        model: "Intel/neural-chat-7b-v3-2",
        name: "Weather Bot",
        tools: [{
            "type": "function",
            "function": {
              "name": "getCurrentWeather",
              "description": "Get the weather in location",
              "parameters": {
                  "type": "object",
                  "properties": {
                  "location": {"type": "string", "description": "The city and state e.g. San Francisco, CA"},
                  "unit": {"type": "string"}
                  },
                  "required": ["location"]
              }
            }
        }]
    });
    console.log(JSON.stringify(assistant, null, 2));
}
createAssistant();
```
```json
{
  "id": "75ce7666-7560-4bb2-8358-48107d94183a",
  "object": "",
  "created_at": 1702071264602,
  "name": "Weather Bot",
  "description": null,
  "model": "Intel/neural-chat-7b-v3-2",
  "instructions": "You are a weather bot. Use the provided functions to answer questions.",
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "getCurrentWeather",
        "description": "Get the weather in location",
        "parameters": {
          "type": "object",
          "properties": {
            "location": {
              "type": "string",
              "description": "The city and state e.g. San Francisco, CA",
              "enum": null
            },
            "unit": {
              "type": "string",
              "description": null,
              "enum": null
            }
          },
          "required": [
            "location"
          ]
        }
      }
    }
  ],
  "file_ids": null,
  "metadata": null,
}
```

3. **Create a Thread**

```ts
async function createThread() {
    const thread = await openai.beta.threads.create();
    console.log(JSON.stringify(thread, null, 2));
}
createThread();
```
```json
{
  "id": "f74681b8-2371-4db1-946f-3efb070f0b19",
  "object": "",
  "created_at": 1702071499412,
  "metadata": null
}
```
4. **Add a Message to a Thread**

*Replace the UUID with the actual thread id*

```ts
async function createMessage() {
    const message = await openai.beta.threads.messages.create(
        "f74681b8-2371-4db1-946f-3efb070f0b19",
        {
            role: "user",
            content: "What's the weather in San Francisco?"
        }
    );
    console.log(JSON.stringify(message, null, 2));
}
createMessage();
```
```json
{
  "id": "7f109eba-796f-4961-9589-3af529f14e6e",
  "object": "",
  "created_at": 1702499213,
  "thread_id": "f74681b8-2371-4db1-946f-3efb070f0b19",
  "role": "user",
  "content": [
    {
      "type": "text",
      "text": {
        "value": "What's the weather in San Francisco?",
        "annotations": []
      }
    }
  ],
  "assistant_id": "00000000-0000-0000-0000-000000000000",
  "run_id": "00000000-0000-0000-0000-000000000000",
  "file_ids": [],
  "metadata": null
}
```
5. **Run the Assistant**

*Replace :thread_id and :assistant_id with the actual thread id and assistant id (check 1. and 3.)*

```ts
async function createRun() {
    const run = await openai.beta.threads.runs.create(
        "f74681b8-2371-4db1-946f-3efb070f0b19",
    { 
        assistant_id: "75ce7666-7560-4bb2-8358-48107d94183a",
        instructions: "You are a weather bot. Use the provided functions to answer questions."
    }
    );
    console.log(JSON.stringify(run, null, 2));
}
createRun();
```
```json
{
  "id": "3933cbe4-5c26-4f93-b2d8-f03dabc055f2",
  "object": "",
  "created_at": 1702499360,
  "thread_id": "f74681b8-2371-4db1-946f-3efb070f0b19",
  "assistant_id": "75ce7666-7560-4bb2-8358-48107d94183a",
  "status": "queued",
  "required_action": null,
  "last_error": null,
  "expires_at": null,
  "started_at": null,
  "cancelled_at": null,
  "failed_at": null,
  "completed_at": null,
  "model": "",
  "instructions": "You are a weather bot. Use the provided functions to answer questions.",
  "tools": [],
  "file_ids": [],
  "metadata": {}
}
```
6. **Check the Run Status**

*Replace :thread_id and :run_id with the actual thread id and run id*

```ts
async function getRun() {
    const run = await openai.beta.threads.runs.retrieve(
        "f74681b8-2371-4db1-946f-3efb070f0b19",
        "3933cbe4-5c26-4f93-b2d8-f03dabc055f2"
    );
    console.log(JSON.stringify(run, null, 2));
}
getRun();
```
(feel free to run this command multiple times until the run is completed - LLM can be slow, especially if you run it on your coffee machine)
```json
{
  "id": "3933cbe4-5c26-4f93-b2d8-f03dabc055f2",
  "object": "",
  "created_at": 1702499360,
  "thread_id": "f74681b8-2371-4db1-946f-3efb070f0b19",
  "assistant_id": "75ce7666-7560-4bb2-8358-48107d94183a",
  "status": "requires_action",
  "required_action": {
    "type": "submit_tool_outputs",
    "submit_tool_outputs": {
      "tool_calls": [
        {
          "id": "9cb246b2-061c-46a4-aa34-80e7587c81c4",
          "type": "function",
          "function": {
            "name": "getCurrentWeather",
            "arguments": "{\"location\":\"San Francisco, CA\",\"unit\":\"imperial\"}"
          }
        }
      ]
    }
  },
  "last_error": null,
  "expires_at": null,
  "started_at": null,
  "cancelled_at": null,
  "failed_at": null,
  "completed_at": null,
  "model": "",
  "instructions": "You are a weather bot. Use the provided functions to answer questions.",
  "tools": [],
  "file_ids": [],
  "metadata": {}
}
```

The Assistant is now waiting for the user to submit the results of the function call.

8. **Submit the Function Call Results**

In practice you would execute, say, your javascript function:

```js
// this would do a request to a weather API
const output = getCurrentWeather({location: "San Francisco, CA", unit: "imperial"})
console.log(output)
> {"temperature": 68, "unit": "F"}
```

Good. So it seems the weather in San Francisco is 68F. Let's tell the LLM about it:

```ts
async function submitToolOutputs() {
    const run = await openai.beta.threads.runs.submitToolOutputs(
        "f74681b8-2371-4db1-946f-3efb070f0b19",
        "3933cbe4-5c26-4f93-b2d8-f03dabc055f2",
        {
            tool_outputs: [
                {
                    tool_call_id: "9cb246b2-061c-46a4-aa34-80e7587c81c4",
                    output: "{\"temperature\": 68, \"unit\": \"F\"}"
                }
            ]
        }
    );
    console.log(JSON.stringify(run, null, 2));
}
submitToolOutputs();
```
```json
{
  "id": "3933cbe4-5c26-4f93-b2d8-f03dabc055f2",
  "object": "",
  "created_at": 1702499360,
  "thread_id": "f74681b8-2371-4db1-946f-3efb070f0b19",
  "assistant_id": "75ce7666-7560-4bb2-8358-48107d94183a",
  "status": "queued",
  "required_action": {
    "type": "submit_tool_outputs",
    "submit_tool_outputs": {
      "tool_calls": [
        {
          "id": "9cb246b2-061c-46a4-aa34-80e7587c81c4",
          "type": "function",
          "function": {
            "name": "getCurrentWeather",
            "arguments": "{\"location\":\"San Francisco, CA\",\"unit\":\"imperial\"}"
          }
        }
      ]
    }
  },
  "last_error": null,
  "expires_at": null,
  "started_at": null,
  "cancelled_at": null,
  "failed_at": null,
  "completed_at": null,
  "model": "",
  "instructions": "You are a weather bot. Use the provided functions to answer questions.",
  "tools": [],
  "file_ids": [],
  "metadata": {}
}
```

Now the LLM knows about the weather in San Francisco, and can answer questions about it.

9. **Display the Assistant's Response**

*Replace with the actual thread id*

```ts
async function getMessages() {
    const messages = await openai.beta.threads.messages.list(
        "f74681b8-2371-4db1-946f-3efb070f0b19"
    );
    console.log(JSON.stringify(messages, null, 2));
}
getMessages();
```
```json
{
  "data": [
    {
      "id": "7f109eba-796f-4961-9589-3af529f14e6e",
      "object": "",
      "created_at": 1702499213,
      "thread_id": "f74681b8-2371-4db1-946f-3efb070f0b19",
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": {
            "value": "What's the weather in San Francisco?",
            "annotations": []
          }
        }
      ],
      "assistant_id": "00000000-0000-0000-0000-000000000000",
      "run_id": "00000000-0000-0000-0000-000000000000",
      "file_ids": [],
      "metadata": null
    },
    {
      "id": "5add8363-1285-463a-a647-6bceee7b84f3",
      "object": "",
      "created_at": 1702499547,
      "thread_id": "f74681b8-2371-4db1-946f-3efb070f0b19",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": {
            "value": "I just checked the weather in San Francisco, CA with Fahrenheit as the unit. The temperature in San Francisco at the given time is 68 degrees Fahrenheit.",
            "annotations": []
          }
        }
      ],
      "assistant_id": "00000000-0000-0000-0000-000000000000",
      "run_id": "00000000-0000-0000-0000-000000000000",
      "file_ids": [],
      "metadata": null
    }
  ]
}
```

