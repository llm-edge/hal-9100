#!/bin/bash

# ! TODO should use id output of request instead of hard coded "1s"

URL=$1

# Create an Assistant
curl -X POST $URL/assistants \
-H "Content-Type: application/json" \
-d '{
    "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
    "name": "Math Tutor",
    "tools": ["retrieval"],
    "model": "claude-2.1"
}'

# Create a Thread
curl -X POST $URL/threads \
-H "Content-Type: application/json"

# Add a Message to a Thread
curl -X POST $URL/threads/1/messages \
-H "Content-Type: application/json" \
-d '{
    "role": "user",
    "content": "I need to solve the equation 3x + 11 = 14. Can you help me?"
}'

# Run the Assistant
curl -X POST $URL/threads/1/runs \
-H "Content-Type: application/json" \
-d '{
    "assistant_id": 1,
    "instructions": "Please solve the equation."
}'

STATUS=""
ATTEMPTS=0
MAX_ATTEMPTS=10

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

