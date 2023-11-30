

At the moment, you need both **Rust** and **Docker** installed to run the API.

Additionally, `Assistants` currently supports Anthropic and Open Source LLMs, you need an API key that you can put in a `.env` file in the root of the project:

```bash
ANTHROPIC_API_KEY="..."
DATABASE_URL=postgres://postgres:secret@localhost:5432/mydatabase
REDIS_URL=redis://127.0.0.1/
S3_ENDPOINT=http://localhost:9000
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
S3_BUCKET_NAME=mybucket
```

## Steps to Run the API

1. **Start the server**

```bash
make all
```

2. **Create an Assistant** 

```bash
curl -X POST http://localhost:3000/assistants \
-H "Content-Type: application/json" \
-d '{
    "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
    "name": "Math Tutor",
    "tools": ["retrieval"],
    "model": "claude-2.1"
}'
```
Result: Assistant created successfully

3. **Create a Thread**

```bash
curl -X POST http://localhost:3000/threads \
-H "Content-Type: application/json"
```
```json
{
    "id": 1,
    "user_id": "user1",
    "file_ids": null,
    "object": "",
    "created_at": 1701039812831,
    "metadata": null
}
```
4. **Add a Message to a Thread**
# Replace 1 with the actual thread id

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
            "type_": "user",
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
    "user_id": "user1"
}
```
5. **Run the Assistant**
# Replace :thread_id and :assistant_id with the actual thread id and assistant id

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
    "user_id": ""
}
```
6. **Check the Run Status**
# Replace :thread_id and :run_id with the actual thread id and run id

```bash
curl -X GET http://localhost:3000/threads/1/runs/1 \
-H "Content-Type: application/json"
```
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
    "user_id": ""
}
```
7. **Display the Assistant's Response**
# Replace :thread_id with the actual thread id

```bash
curl http://localhost:3000/threads/1/messages \
-H "Content-Type: application/json"
```
```json
[
    {
        "id": 1,
        "object": "",
        "created_at": 1701039816652,
        "thread_id": 1,
        "role": "user",
        "content": [
            {
                "type_": "user",
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
        "user_id": "user1"
    },
    {
        "id": 2,
        "object": "",
        "created_at": 1701039826151,
        "thread_id": 1,
        "role": "assistant",
        "content": [
            {
                "type_": "text",
                "text": {
                    "value": " Unfortunately I do not have enough context to solve the equation. Please provide the full equation you would like me to solve, and I will do my best to assist you in solving it. Some examples of helpful context that would allow me to solve an equation are:\n\n- Specifying the full equation, including all variables, numbers, and mathematical operators (+, - , /, , =, etc)\n- Providing any constraints or requirements on the variables\n- Specifying the desired form of the solution",
                    "annotations": []
                }
            }
        ],
        "assistant_id": null,
        "run_id": null,
        "file_ids": null,
        "metadata": null,
        "user_id": ""
    }
]
```