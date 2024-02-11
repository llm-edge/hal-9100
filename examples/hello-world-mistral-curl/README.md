

At the moment, you need **Docker** installed to run the API.

Additionally, `HAL-9100` currently supports Anthropic and Open Source LLMs, you need some env vars that you can put in a `.env` file in the root of the project:

```bash
DATABASE_URL=postgres://postgres:secret@localhost:5432/mydatabase
REDIS_URL=redis://127.0.0.1/
S3_ENDPOINT=http://localhost:9000
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
S3_BUCKET_NAME=mybucket
MODEL_URL="http://host.docker.internal:8000/v1/chat/completions"
```

Please install `jq` if you haven't already. You can install it using `brew install jq` on MacOS or `sudo apt-get install` jq on Ubuntu.


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
assistant_response=$(curl -sS -X POST http://localhost:3000/assistants \
-H "Content-Type: application/json" \
-d '{
    "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
    "name": "Math Tutor",
    "tools": [],
    "model": "open-orca/mistral-7b-openorca"
}')
echo $assistant_response
assistant_id=$(echo $assistant_response | jq -r '.id')
```
```json
{
    "id": "f498889a-165d-455f-b2bd-152540072359",
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
thread_response=$(curl -sS -X POST http://localhost:3000/threads \
-H "Content-Type: application/json")
echo $thread_response
thread_id=$(echo $thread_response | jq -r '.id')
```
```json
{
    "id": "7c9396ee-258b-4c4e-b656-92745a4f1ccb",
    "file_ids": null,
    "object": "",
    "created_at": 1701039812831,
    "metadata": null
}
```
4. **Add a Message to a Thread**


```bash
message_response=$(curl -sS -X POST http://localhost:3000/threads/$thread_id/messages \
-H "Content-Type: application/json" \
-d '{
    "role": "user",
    "content": "I need to solve the equation 3x + 11 = 14. Can you help me?"
}')
echo $message_response
message_id=$(echo $message_response | jq -r '.id')
```
```json
{
    "id": "e2f10813-9763-486d-9e71-eacc9c97cf3e",
    "object": "",
    "created_at": 1701039816652,
    "thread_id": "7c9396ee-258b-4c4e-b656-92745a4f1ccb",
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

```bash
run_response=$(curl -sS -X POST http://localhost:3000/threads/$thread_id/runs \
-H "Content-Type: application/json" \
-d '{
    "assistant_id": "'$assistant_id'",
    "instructions": "Please solve the equation."
}')
echo $run_response
run_id=$(echo $run_response | jq -r '.id')
```
```json
{
    "id": "e2f10813-9763-486d-9e71-eacc9c97cf3e",
    "object": "",
    "created_at": 1701039820804,
    "thread_id": "7c9396ee-258b-4c4e-b656-92745a4f1ccb",
    "assistant_id": "f498889a-165d-455f-b2bd-152540072359",
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


```bash
curl -sS -X GET http://localhost:3000/threads/$thread_id/runs/$run_id \
-H "Content-Type: application/json"
```
(feel free to run this command multiple times until the run is completed - LLM can be slow, especially if you run it on your coffee machine)
```json
{
    "id": "e2f10813-9763-486d-9e71-eacc9c97cf3e",
    "object": "",
    "created_at": 1701039820804,
    "thread_id": "7c9396ee-258b-4c4e-b656-92745a4f1ccb",
    "assistant_id": "f498889a-165d-455f-b2bd-152540072359",
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


```bash
curl -sS http://localhost:3000/threads/$thread_id/messages \
-H "Content-Type: application/json"
```
```json
[
    {
        "id": "e2f10813-9763-486d-9e71-eacc9c97cf3e",
        "object": "",
        "created_at": 1701301908671,
        "thread_id": "7c9396ee-258b-4c4e-b656-92745a4f1ccb",
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
        "id": "e2f10813-9763-486d-9e71-eacc9c97cf3e",
        "object": "",
        "created_at": 1701302114890,
        "thread_id": "7c9396ee-258b-4c4e-b656-92745a4f1ccb",
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
