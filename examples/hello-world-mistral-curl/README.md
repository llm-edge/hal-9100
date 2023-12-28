

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

## Steps to Run the API

1. **Run Mistral open source LLM**

We'll be using [FastChat](https://github.com/lm-sys/FastChat) to run the LLM, but many options are available, [let me know if you need help or want to run this in your infra](mailto:hi@louis030195.com).

Assuming you have Python 3 and virtualenv installed.

On MacOS M1/M2:

```bash
virtualenv env
source env/bin/activate
pip3 install "fschat[model_worker]"

# Terminal 1
python3 -m fastchat.serve.controller

# Terminal 2
python3 -m fastchat.serve.model_worker --model-path open-orca/mistral-7b-openorca --device mps --load-8bit

# Terminal 3
python3 -m fastchat.serve.openai_api_server --host localhost --port 8000

# Terminal 4
# Test if it works properly:
curl http://localhost:8000/v1/chat/completions   -H "Content-Type: application/json"   -d '{"model": "mistral-7b-openorca","messages": [{"role": "user", "content": "Hello! What is your name?"}]}' 
```

1. **Start the server**

```bash
docker-compose --profile api -f docker/docker-compose.yml up -d
```

2. **Create an Assistant** 

```bash
curl -X POST http://localhost:3000/assistants \
-H "Content-Type: application/json" \
-d '{
    "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
    "name": "Math Tutor",
    "tools": [{"type": "retrieval"}],
    "model": "open-orca/mistral-7b-openorca"
}'
```
```json
{
    "id": 1,
    "object": "",
    "created_at": 1701298908915,
    "name": "Math Tutor",
    "description": null,
    "model": "open-orca/mistral-7b-openorca",
    "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
    "tools": [{"type": "retrieval"}],
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
    "content": "I need to solve the equation 3x + 11 = 14. Can you help me?"
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
                "value": "I need to solve the equation 3x + 11 = 14. Can you help me?",
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
    "instructions": "Please solve the equation."
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
    "instructions": "Please solve the equation.",
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
(feel free to run this command multiple times until the run is completed - LLM can be slow, especially if you run it on your coffee machine)
```json
{
    "id": 1,
    "object": "",
    "created_at": 1701039820804,
    "thread_id": 1,
    "assistant_id": 1,
    "status": "running",
    "required_action": null,
    "last_error": null,
    "expires_at": 0,
    "started_at": null,
    "cancelled_at": null,
    "failed_at": null,
    "completed_at": null,
    "model": "",
    "instructions": "Please solve the equation.",
    "tools": [],
    "file_ids": [],
    "metadata": null,
}
```
7. **Display the Assistant's Response**

*Replace :thread_id with the actual thread id*

```bash
curl http://localhost:3000/threads/1/messages \
-H "Content-Type: application/json"
```
```json
[
    {
        "id": 1,
        "object": "",
        "created_at": 1701301908671,
        "thread_id": 1,
        "role": "user",
        "content": [
            {
                "type": "user",
                "text": {
                    "value": "I need to solve the equation 3x + 11 = 14. Can you help me?",
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
        "created_at": 1701302114890,
        "thread_id": 1,
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": {
                    "value": "To solve the equation 3x + 11 = 14, we need to isolate the variable x. Here's the step-by-step reasoning:\n\n1. Our goal is to find the value of x that makes the equation true.\n2. First, let's subtract 11 from both sides of the equation to isolate the term with the variable (3x) on one side:\n   3x + 11 - 11 = 14 - 11\n   \n   This simplifies to:\n   3x = 3\n\n3. Now, divide both sides of the equation by 3 to get the value of x:\n   (3x) / 3 = 3 / 3\n\n   This simplifies to:\n   x = 1\n\nSo the solution to the equation is x = 1.",
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
