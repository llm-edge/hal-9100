const OpenAI = require('openai');
const fs = require('fs');

const openai = new OpenAI({
    baseURL: 'http://localhost:3000',
    apiKey: 'EMPTY',
});

async function getVCCapital() {
    return { my_capital_range_i_want_to_invest_in_next_round: "$10K to $1B" };
}

async function uploadFile() {
    // Upload a file with an "assistants" purpose
    const response = await openai.files.create({
        file: fs.createReadStream("./examples/hello-world-code-interpreter-mixtral-nodejs/startup_data.csv"),
        purpose: "assistants",
    });

    console.log(JSON.stringify(response, null, 2));
    return response.file_id;
}

async function createAssistant(fileId) {
    const assistant = await openai.beta.assistants.create({
        instructions: "You are a VC copilot. Write and run code to answer questions about startups investment.",
        model: ENV_MODEL_NAME,
        name: "VC Copilot",
        tools: [{
            "type": "function",
            "function": {
                "name": "getVCCapital",
                "description": "Get the capital of the VC firm. Use this function if the user wants to invest but you don't know his capital capacities.",
                "parameters": {
                    "type": "object"
                }
            }
        }, {
            "type": "code_interpreter",
        }],
        file_ids: [fileId]
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
            content: "Which startup should I invest in based on my capital? Just say the startup name and I'll drop the check and go back sip cocktails on the beach."
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
            instructions: "You are a VC copilot.",
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
    const capital = await getVCCapital(args);
    const run = await openai.beta.threads.runs.submitToolOutputs(
        threadId,
        runId,
        {
            tool_outputs: [
                {
                    tool_call_id: toolCallId,
                    output: JSON.stringify(capital)
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
    const fileId = await uploadFile();
    const assistant = await createAssistant(fileId);
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