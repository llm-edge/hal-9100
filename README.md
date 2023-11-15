
# Open Source Assistants API Documentation

## Overview
The Open Source Assistants API allows you to build AI assistants within your applications, leveraging models, tools, and knowledge to respond to user queries. This API is currently in beta, with plans to add more OpenAI-built tools and support for user-provided tools.

### Features
- **Code Interpreter**: Executes Python code in a sandbox.
- **Knowledge Retrieval**: Augments the Assistant with external knowledge or documents.
- **Function Calling**: Define and execute custom functions.
- **File Handling**: Supports various file formats for input and output.

## Getting Started
1. **Create an Assistant**: Define custom instructions, pick a model, and enable tools like Code Interpreter, Retrieval, and Function calling.
2. **Create a Thread**: Represents a user's conversation, store messages and context.
3. **Add Messages**: Text or files added to the conversation thread.
4. **Run the Assistant**: Trigger responses using the model and tools.
5. **Manage Outputs**: Handle file paths and citations in Assistant's responses.

### Example Usage
```python
# Creating an Assistant
assistant = client.beta.assistants.create(
    name="Math Tutor",
    instructions="Solve math questions using code.",
    tools=[{"type": "code_interpreter"}],
    model="gpt-4-1106-preview"
)

# Creating a Thread
thread = client.beta.threads.create()

# Adding a Message
message = client.beta.threads.messages.create(
    thread_id=thread.id,
    role="user",
    content="Solve the equation `3x + 11 = 14`."
)

# Running the Assistant
run = client.beta.threads.runs.create(
  thread_id=thread.id,
  assistant_id=assistant.id,
  instructions="Additional instructions."
)
```

## Data Access and Security
- Implement strict authorization checks.
- Restrict API key access within your organization.
- Consider creating separate accounts for different applications.

## Limitations and Future Developments
- Currently, in beta with known limitations.
- Future plans include support for streaming output, notifications, DALL·E integration, and image handling in user messages.

## Next Steps
- Learn more about [How Assistants Work](#how-assistants-work).
- Explore the [Assistants Playground](#assistants-playground).
- Dive deeper into [Tools](#tools).

---

# How Assistants Work

## Core Concepts
- **Assistants**: Use OpenAI’s models and tools to perform tasks.
- **Threads**: Store conversation history, handling truncation for model context.
- **Messages**: Text, images, and files exchanged between users and Assistants.
- **Runs**: Invocations of Assistants on Threads.
- **Run Steps**: Detailed steps taken by Assistants during Runs.

## Creating Assistants
- Use latest OpenAI models for compatibility.
- Customize behavior with `instructions`, `tools`, and `file_ids` parameters.
- Upload files for use with Assistants.

## Managing Threads and Messages
- No limit on number of Messages in a Thread.
- Automatic truncation to fit model context.
- Support for text, images, and files (future support for user-created image messages).

## Annotations in Messages
- Handle `file_citation` and `file_path` annotations.
- Replace model-generated substrings with annotations for clarity.

## Run and Run Steps
- Monitor Run status for application flow control.
- Examine Run Steps for insights into Assistant's decision-making process.

## Data Access Guidelines
- Implement robust authorization and access controls.
- Separate accounts for different applications for data isolation.

## Known Limitations and Upcoming Features
- Streaming output and notifications.
- DALL·E integration.
- Image handling in user messages.
