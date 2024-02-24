
Before starting, make sure you have a modern laptop or desktop with at least 16GB of RAM and a modern CPU. You will also need to have Node.js installed.

This LLM runs at 60+ tokens per second on my MacBook Pro M2.

At the moment, you need **Docker** installed to run the API.

You need to update the config `hal-9100.toml` file in the root of the project:

```bash
model_url="http://host.docker.internal:8000/v1/chat/completions"
```

We will use [OpenAI's JS SDK](https://github.com/openai/openai-node), but feel free to use the [python one](https://github.com/openai/openai-python), you can copy paste this doc in chatgpt to translate to python!

# Action tool

Action allows you to describe an API to the Assistants and have it intelligently do requests autonomously to the API. This is a powerful feature that allows you to build Assistants that can interact with the world in a meaningful way.

We'll use `Mistral-7B-Instruct-v0.2-q4f16_1-MLC` which is one of the best 7b sized open source LLM to this date, thanks to Mistral!

## Steps to Run the API

1. **Run Mistral open source LLM**

We'll be using [MLC-LLM](https://github.com/mlc-ai/mlc-llm) to run the LLM, but many options are available, [let me know if you need help or want to run this in your infra](mailto:hi@louis030195.com).

Assuming you have Python 3 and virtualenv installed.

```bash
# Install git-lfs, on Mac:
brew install git-lfs
# On Ubuntu:
sudo apt-get install git-lfs
git lfs install

# Create a virtualenv
virtualenv env
source env/bin/activate

# Install mlc-llm
# On Mac:
python3 -m pip install --pre -U -f https://mlc.ai/wheels mlc-chat-nightly mlc-ai-nightly
# Otherwise check: https://llm.mlc.ai/docs/install/mlc_llm.html

mkdir -p dist
git clone https://github.com/mlc-ai/binary-mlc-llm-libs.git dist/prebuilt_libs
MODEL="Mistral-7B-Instruct-v0.2-q4f16_1-MLC"
cd dist && git clone https://huggingface.co/mlc-ai/$MODEL
python -m mlc_chat.rest --model $MODEL --port 8000 --host 0.0.0.0
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
    baseURL: 'http://localhost:8000/v1',
    apiKey: 'EMPTY',
});

async function main() {
  const chatCompletion = await openai.chat.completions.create({
    messages: [{ role: 'user', content: 'Hello! What is your name?' }],
    model: 'Mistral-7B-Instruct-v0.2-q4f16_1-MLC',
  });
  console.log(chatCompletion.choices[0].message.content);
}

main();
```

You should see something like this:
> I don't have a name. I'm here to help answer any questions you have to the best of my ability. Let me know how I can assist you today.

## Actions with Mistral's LLM in JS

1. **Start the server**

```bash
docker compose --profile api -f docker/docker-compose.yml up -d
```

2. **Create an Assistant** 

Alright, open a new terminal and start `node`, then drop these snippets step by step:

```ts
const OpenAI = require('openai');

const openai = new OpenAI({
    baseURL: 'http://localhost:3000',
    apiKey: 'EMPTY',
});

let threadId, assistantId, runId;
```

Let's define an OpenAPI spec for a simple Wikipedia API:

```ts
const openapi_spec = `
openapi: 3.0.0
info:
  title: MediaWiki Random API
  description: This API returns a set of random pages from MediaWiki.
  version: 1.0.0
servers:
  - url: https://en.wikipedia.org/w
    description: Wikipedia API Server
paths:
  /api.php:
    get:
      operationId: getRandomPages
      summary: Get a set of random pages
      description: Returns a list of random pages from MediaWiki.
      parameters:
        - name: action
          in: query
          required: true
          description: The action to perform.
          schema:
            type: string
            default: query
        - name: format
          in: query
          required: true
          description: The format of the output.
          schema:
            type: string
            default: json
        - name: list
          in: query
          required: true
          description: Specify the list as random.
          schema:
            type: string
            default: random
        - name: rnnamespace
          in: query
          required: false
          description: Return pages in these namespaces only.
          schema:
            type: string
        - name: rnfilterredir
          in: query
          required: false
          description: How to filter for redirects.
          schema:
            type: string
            enum: [all, nonredirects, redirects]
            default: nonredirects
        - name: rnlimit
          in: query
          required: false
          description: Limit how many random pages will be returned.
          schema:
            type: integer
            default: 1
            minimum: 1
            maximum: 500
      responses:
        '200':
          description: A list of random pages
          content:
            application/json:
              schema: 
                type: object
                properties:
                  batchcomplete:
                    type: string
                  continue:
                    type: object
                    properties:
                      rncontinue:
                        type: string
                      continue:
                        type: string
                  query:
                    type: object
                    properties:
                      random:
                        type: array
                        items:
                          type: object
                          properties:
                            id:
                              type: integer
                            ns:
                              type: integer
                            title:
                              type: string
`;
```

```ts
async function createAssistant() {
    const assistant = await openai.beta.assistants.create({
        name: "Todo Helper",
        instructions: "You are an assistant that gives random Wikipedia pages.",
        tools: [
            {
                "type": "action",
                "data": {"openapi_spec": openapi_spec},
            }
        ],
        model: "mistralai/Mistral-7B-Instruct-v0.2-q4f16_1-MLC"
    });
    assistantId = assistant.id;
    console.log(JSON.stringify(assistant, null, 2));
}

createAssistant();
```
```json
{
  "id": "7e8a9a01-1d5e-445f-a3d9-fdea9f2ac5e0",
  "object": "",
  "created_at": 1707835077,
  "name": "Todo Helper",
  "description": null,
  "model": "mistralai/Mistral-7B-Instruct-v0.2-q4f16_1-MLC",
  "instructions": "You are an assistant that gives random Wikipedia pages.",
  "tools": [
    {
      "type": "action",
      "data": {
        "openapi_spec": "..."
      }
    }
  ],
  "file_ids": [],
  "metadata": null
}
```

3. **Create a Thread**

```ts
async function createThread() {
    const thread = await openai.beta.threads.create();
    threadId = thread.id;
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

```ts
async function createMessage() {
    const message = await openai.beta.threads.messages.create(
        threadId,
        {
            role: "user",
            content: "Give me a page",
        }
    );
    console.log(JSON.stringify(message, null, 2));
}
createMessage();
```
```json
{
  "id": "fa5b0000-36f8-4937-ac0f-a3b8f78b4022",
  "object": "",
  "created_at": 1707834584,
  "thread_id": "6280bcd0-07ba-4094-ac11-d5fac7322d78",
  "role": "user",
  "content": [
    {
      "type": "text",
      "text": {
        "value": "Give me a todo",
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

```ts
async function createRun() {
    const run = await openai.beta.threads.runs.create(
        threadId,
    { 
        assistant_id: assistantId,
        instructions: "You are a wikipedia bot. Use the provided action to get random pages."
    }
    );
    runId = run.id;
    console.log(JSON.stringify(run, null, 2));
}
createRun();
```
```json
{
  "id": "f79f92a2-a55e-4f9d-b61f-951e77f0d9c8",
  "object": "",
  "created_at": 1707834623,
  "thread_id": "6280bcd0-07ba-4094-ac11-d5fac7322d78",
  "assistant_id": "09ba0471-0548-4777-9595-4387fd1d553f",
  "status": "queued",
  "required_action": null,
  "last_error": null,
  "expires_at": null,
  "started_at": null,
  "cancelled_at": null,
  "failed_at": null,
  "completed_at": null,
  "model": "",
  "instructions": "You are a todo bot. Use the provided action to get todos.",
  "tools": [],
  "file_ids": [],
  "metadata": {}
}
```
6. **Check the Run Status**

```ts
async function getRun() {
    const run = await openai.beta.threads.runs.retrieve(
        threadId,
        runId
    );
    console.log(JSON.stringify(run, null, 2));
}
getRun();
```
(feel free to run this command multiple times until the run is completed - LLM can be slow, especially if you run it on your coffee machine)
```json
{
  "id": "aa192ed1-b716-4f93-9eb4-7f36f760220c",
  "object": "",
  "created_at": 1707835116,
  "thread_id": "27e8b3d2-9d69-46a8-94ab-d0e6b4bff361",
  "assistant_id": "7e8a9a01-1d5e-445f-a3d9-fdea9f2ac5e0",
  "status": "completed",
  "required_action": null,
  "last_error": null,
  "expires_at": null,
  "started_at": null,
  "cancelled_at": null,
  "failed_at": null,
  "completed_at": null,
  "model": "",
  "instructions": "You are a wikipedia bot. Use the provided action to get random pages.",
  "tools": [],
  "file_ids": [],
  "metadata": {}
}
```

7. **Display the Assistant's Response**

```ts
async function getMessages() {
    const messages = await openai.beta.threads.messages.list(
        threadId
    );
    console.log(JSON.stringify(messages, null, 2));
}
getMessages();
```
```json
{
  ...
  },
  "data": [
    {
      "id": "d93c237f-3e4d-479a-9a07-47b4253e886a",
      "object": "",
      "created_at": 1707835109,
      "thread_id": "27e8b3d2-9d69-46a8-94ab-d0e6b4bff361",
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": {
            "value": "Give me a page",
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
      "id": "d59846b0-ea3b-4eaf-af4b-d7e9f29edd0a",
      "object": "",
      "created_at": 1707835170,
      "thread_id": "27e8b3d2-9d69-46a8-94ab-d0e6b4bff361",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": {
            "value": "The random Wikipedia page I can provide you with is about \"Talk:China Household Finance Survey\". Please note that I cannot directly access or show the content of the page, but I can provide you with the title of the page if you'd like. Let me know if that works for you. If you'd like a different page, just let me know and I'll get you another random one.",
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

