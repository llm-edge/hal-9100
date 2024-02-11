

At the moment, you need both **Docker** installed to run the API.

Additionally, we'll use Anthropic LLM here for simplicity, you need an API key that you can put in a `.env` file in the root of the project:

```bash
ANTHROPIC_API_KEY="..."
DATABASE_URL=postgres://postgres:secret@localhost:5432/mydatabase
REDIS_URL=redis://127.0.0.1/
S3_ENDPOINT=http://localhost:9000
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
S3_BUCKET_NAME=mybucket
```

# Function calling

Function calling allows you to describe functions to the Assistants and have it intelligently return the functions that need to be called along with their arguments. The Assistants API will pause execution during a Run when it invokes functions, and you can supply the results of the function call back to continue the Run execution.

## Steps to Run the API

1. **Start the server**

```bash
docker compose --profile api -f docker/docker-compose.yml up -d
```

2. **Create an Assistant** 

```bash
curl -X POST http://localhost:3000/assistants \
-H "Content-Type: application/json" \
-d '{
    "instructions": "You are a cosmic calculator. Crunch numbers to find the purpose of the universe.",
    "name": "Cosmic Calculator",
    "tools": [{
      "type": "function",
      "function": {
        "name": "getPurpose",
        "description": "Compute the purpose of the universe",
        "parameters": {
          "type": "object",
          "properties": {
            "location": {"type": "string", "description": "Inflation of your universe", "enum": null}
          },
          "required": ["location"]
        }
      }	
    }],
    "model": "claude-2.1"
}'
```
```json
{
    "id": 1,
    "object": "",
    "created_at": 1702008077179,
    "name": "Cosmic Calculator",
    "description": null,
    "model": "claude-2.1",
    "instructions": "You are a cosmic calculator. Crunch numbers to find the purpose of the universe.",
    "tools": [
        {
            "type": "function",
            "function": {
                "name": "getPurpose",
                "description": "Compute the purpose of the universe",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "Inflation of your universe",
                            "enum": null
                        }
                    },
                    "required": ["location"]
                }
            }
        }
    ],
    "file_ids": null,
    "metadata": null,
}
```

3. **Create a Thread**

```bash
curl -X POST http://localhost:3000/threads \
-H "Content-Type: application/json"
```
```json
{
    "id": 1,
    "file_ids": null,
    "object": "",
    "created_at": 1701039812831,
    "metadata": null
}
```
4. **Add a Message to a Thread**

*Replace 1 with the actual thread id*

```bash
curl -X POST http://localhost:3000/threads/1/messages \
-H "Content-Type: application/json" \
-d '{
    "role": "user",
    "content": "I need to understand the purpose of the universe. If you do not know I am going to die."
}'
```
```json
{
    "id": 1,
    "object": "",
    "created_at": 1701039816652,
    "thread_id": 1,
    "role": "user",
    "content": [
        {
            "type": "user",
            "text": {
                "value": "I need to understand the purpose of the universe. If you do not know I am going to die.",
                "annotations": []
            }
        }
    ],
    "assistant_id": null,
    "run_id": null,
    "file_ids": null,
    "metadata": null,
}
```
5. **Run the Assistant**

*Replace :thread_id and :assistant_id with the actual thread id and assistant id*

```bash
curl -X POST http://localhost:3000/threads/1/runs \
-H "Content-Type: application/json" \
-d '{
    "assistant_id": 1,
    "instructions": "Please solve the purpose of the universe."
}'
```
```json
{
    "id": 1,
    "object": "",
    "created_at": 1701039820804,
    "thread_id": 1,
    "assistant_id": 1,
    "status": "queued",
    "required_action": null,
    "last_error": null,
    "expires_at": 0,
    "started_at": null,
    "cancelled_at": null,
    "failed_at": null,
    "completed_at": null,
    "model": "",
    "instructions": "Please solve the purpose of the universe.",
    "tools": [],
    "file_ids": [],
    "metadata": null,
}
```
6. **Check the Run Status**

*Replace :thread_id and :run_id with the actual thread id and run id*

```bash
curl -X GET http://localhost:3000/threads/1/runs/1 \
-H "Content-Type: application/json"
```
```json
{
    "id": 1,
    "object": "",
    "created_at": 1702008329585,
    "thread_id": 1,
    "assistant_id": 1,
    "status": "requires_action",
    "required_action": {
        "type": "submit_tool_outputs",
        "submit_tool_outputs": {
            "tool_calls": [
                {
                    "id": "6b674f1d-07e4-4960-8317-17e669dcfba2",
                    "type": "function",
                    "function": {
                        "name": "getPurpose",
                        "arguments": {
                            "location": "the universe"
                        }
                    }
                }
            ]
        }
    },
    "last_error": null,
    "expires_at": 0,
    "started_at": null,
    "cancelled_at": null,
    "failed_at": null,
    "completed_at": null,
    "model": "",
    "instructions": "Please solve the purpose of the universe.",
    "tools": [],
    "file_ids": [],
    "metadata": {},
}
```

The Assistant is now waiting for the user to submit the results of the function call.

8. **Submit the Function Call Results**

In practice you would execute, say, your javascript function:

```js
const output = getPurpose({location: "the universe"})
console.log(output)
> "The purpose of the universe is to be a cosmic calculator. PS: 42"
```

Good. So it seems the purpose of the universe is 42. Let's submit that to the Assistant:

```bash
TOOL_CALL_ID="REPLACE ME FROM PREVIOUS CALL RESULT"
curl http://localhost:3000/threads/1/runs/1/submit_tool_outputs \
-H "Content-Type: application/json" \
-d '{
"tool_outputs": [{
    "tool_call_id": "$TOOL_CALL_ID",
    "output": "The purpose of the universe is to be a cosmic calculator. PS: 42"
}]
}'
```
```json 
{
    "id": 1,
    "object": "",
    "created_at": 1702008329585,
    "thread_id": 1,
    "assistant_id": 1,
    "status": "queued",
    "required_action": {
        "type": "submit_tool_outputs",
        "submit_tool_outputs": {
            "tool_calls": [
                {
                    "id": "6b674f1d-07e4-4960-8317-17e669dcfba2",
                    "type": "function",
                    "function": {
                        "name": "getPurpose",
                        "arguments": {
                            "location": "the universe"
                        }
                    }
                }
            ]
        }
    },
    "last_error": null,
    "expires_at": 0,
    "started_at": null,
    "cancelled_at": null,
    "failed_at": null,
    "completed_at": null,
    "model": "",
    "instructions": "Please solve the purpose of the universe.",
    "tools": [],
    "file_ids": [],
    "metadata": {},
}
```


9. **Display the Assistant's Response**

*Replace 1 with the actual thread id*

```bash
curl http://localhost:3000/threads/1/messages \
-H "Content-Type: application/json"
```
```json
[
    {
        "id": 1,
        "object": "",
        "created_at": 1702008304303,
        "thread_id": 1,
        "role": "user",
        "content": [
            {
                "type": "user",
                "text": {
                    "value": "I need to understand the purpose of the universe. If you do not know I am going to die.",
                    "annotations": []
                }
            }
        ],
        "assistant_id": null,
        "run_id": null,
        "file_ids": null,
        "metadata": null,
    },
    {
        "id": 2,
        "object": "",
        "created_at": 1702008774388,
        "thread_id": 1,
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": {
                    "value": " I do not have enough information or capability to definitively state the purpose of the universe. As an AI assistant without omniscience, I can only speculate based on available evidence and philosophical perspectives. However, I apologize that I cannot provide a definitive answer that would prevent your death in this hypothetical scenario. Perhaps we could have a thoughtful discussion about what gives life meaning from your perspective.",
                    "annotations": []
                }
            }
        ],
        "assistant_id": null,
        "run_id": null,
        "file_ids": null,
        "metadata": null,
    }
]
```

