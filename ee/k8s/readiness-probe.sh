#!/bin/bash

URL=$1

# Create an Assistant
ASSISTANT_RESPONSE=$(curl -s -X POST $URL/assistants \
-H "Content-Type: application/json" \
-d '{
    "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
    "name": "Math Tutor",
    "tools": [{"type": "retrieval"}],
    "model": "mixtral-8x7b-instruct"
}')

ASSISTANT_ID=$(echo $ASSISTANT_RESPONSE | jq -r '.id')

# Create a Thread
THREAD_RESPONSE=$(curl -s -X POST $URL/threads \
-H "Content-Type: application/json")

THREAD_ID=$(echo $THREAD_RESPONSE | jq -r '.id')

# Add a Message to a Thread
curl -X POST $URL/threads/$THREAD_ID/messages \
-H "Content-Type: application/json" \
-d '{
    "role": "user",
    "content": "I need to solve the equation 3x + 11 = 14. Can you help me?"
}'

# Run the Assistant
curl -X POST $URL/threads/$THREAD_ID/runs \
-H "Content-Type: application/json" \
-d '{
    "assistant_id": "'$ASSISTANT_ID'",
    "instructions": "Please solve the equation."
}'

STATUS=""
ATTEMPTS=0
MAX_ATTEMPTS=3

# Poll until status is succeeded or max attempts reached
while [ "$STATUS" != "succeeded" -a $ATTEMPTS -lt $MAX_ATTEMPTS ]; do
    RESPONSE=$(curl -s -X GET $URL/threads/1/runs/1 -H "Content-Type: application/json")
    STATUS=$(echo $RESPONSE | jq -r '.status')
    sleep 5
    let ATTEMPTS=ATTEMPTS+1
done

if [ "$STATUS" != "succeeded" ]; then
    echo "Status is $STATUS after $MAX_ATTEMPTS attempts, failing readiness probe"
    exit 1
else
    echo "Status is $STATUS"
fi

