

// This example demonstrates how to use the Open Source Assistants API to create an assistant that can answer questions about the weather.
// 1. Get an API key from Perplexity https://docs.perplexity.ai/docs
// and place in .env MODEL_API_KEY="get it here https://docs.perplexity.ai/docs"
// 2. Start Postgres, Redis, and Minio:
// make reboot
// 3. In another terminal, start the API server:
// make all
// 4. In another terminal, install OpenAI SDK:
// npm i openai
// 5. Run this script:
// node ./examples/quickstart.js

const OpenAI = require('openai');

const openai = new OpenAI({
    baseURL: 'http://localhost:3000',
    apiKey: 'EMPTY',
});

async function getCurrentWeather(location) {
    // TODO: fetch weather from some API
    return { temperature: "68", unit: "F" };
}

async function createAssistant() {
    const assistant = await openai.beta.assistants.create({
        instructions: "You are a weather bot. Use the provided functions to answer questions.",
        model: "mistralai/mixtral-8x7b-instruct",
        name: "Weather Bot",
        tools: [{
            "type": "function",
            "function": {
                "name": "getCurrentWeather",
                "description": "Get the weather in location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": { "type": "string", "description": "The city and state e.g. San Francisco, CA" },
                        "unit": { "type": "string" }
                    },
                    "required": ["location"]
                }
            }
        }]
    });
    console.log(JSON.stringify(assistant, null, 2));
    return assistant;
}


async function createThread() {
    const thread = await openai.beta.threads.create();
    console.log(JSON.stringify(thread, null, 2));
    return thread;
}

async function createMessage(threadId) {
    const message = await openai.beta.threads.messages.create(
        threadId,
        {
            role: "user",
            content: "What's the weather in San Francisco?"
        }
    );
    console.log(JSON.stringify(message, null, 2));
    return message;
}

async function createRun(threadId, assistantId) {
    const run = await openai.beta.threads.runs.create(
        threadId,
        {
            assistant_id: assistantId,
            instructions: "You are a weather bot. Use the provided functions to answer questions."
        }
    );
    console.log(JSON.stringify(run, null, 2));
    return run;
}

async function getRun(threadId, runId) {
    const run = await openai.beta.threads.runs.retrieve(threadId, runId);
    console.log(JSON.stringify(run, null, 2));
    return run;
}

async function submitToolOutputs(threadId, runId, toolCallId, args) {
    const weather = await getCurrentWeather(args.location);
    const run = await openai.beta.threads.runs.submitToolOutputs(
        threadId,
        runId,
        {
            tool_outputs: [
                {
                    tool_call_id: toolCallId,
                    output: JSON.stringify(weather)
                }
            ]
        }
    );
    console.log(JSON.stringify(run, null, 2));
    return run;
}

async function getMessages(threadId) {
    const messages = await openai.beta.threads.messages.list(threadId);
    console.log(JSON.stringify(messages, null, 2));
    return messages;
}

async function main() {
    const assistant = await createAssistant();
    const thread = await createThread();
    const threadId = thread.id;
    await createMessage(threadId);
    const run = await createRun(threadId, assistant.id);
    let runStatus;
    const intervalId = setInterval(async () => {
        runStatus = await getRun(threadId, run.id);
        if (runStatus.status === 'requires_action') {
            clearInterval(intervalId);
            const toolCall = runStatus.required_action.submit_tool_outputs.tool_calls[0];
            await submitToolOutputs(threadId, run.id, toolCall.id, JSON.parse(toolCall.function.arguments));
            let messages;
            const messageIntervalId = setInterval(async () => {
                messages = await getMessages(threadId);
                console.log(JSON.stringify(messages, null, 2));
                if (messages.data.some(message => message.role === 'assistant')) {
                    clearInterval(messageIntervalId);
                }
            }, 1000);
        }
    }, 1000);
}

main();