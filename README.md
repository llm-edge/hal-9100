

<p align="center">
<img width="150" alt="assistants" src="https://github.com/louis030195/assistants/assets/25003283/499b65e0-38fb-464b-a4d3-bb7f83f2a81b">
  <h1 align="center">⭐️ Open Source ⭐️ <s>OpenAI</s> Assistants API</h1>

  <h3 align="center">The ⭐️ Open Source ⭐️ <s>OpenAI</s> Assistants API allows you to build AI assistants within your own applications</h3>

  <p align="center">
    <br />
    <a href="https://discord.gg/xxx"><img alt="Discord" src="https://img.shields.io/discord/1066022656845025310?color=black&style=for-the-badge"></a>
    <br />
    <div align="center">
      <a href="stripelink">💰 Pre-order the commercial license 10x cheaper now</a>
      <br />
      <a href="https://github.com/louis030195/assistants/issues/new?assignees=&labels=enhancement">✨ Request Feature</a>
      <br />
      <a href="https://github.com/louis030195/assistants/issues/new?assignees=&labels=bug">❤️‍🩹 Report Bug</a>
    </div>
    <br />
  </p>
</p>

# Open Source Assistants API Documentation (wip)

## Overview
The Open Source Assistants API enables building AI assistants within applications using models, tools, and knowledge to respond to user queries. This API is in beta, with continuous enhancements and support for various tools.

### Key Features
- **Code Interpreter**: Runs Python code in a sandboxed environment.
- **Knowledge Retrieval**: Retrieves external knowledge or documents.
- **Function Calling**: Defines and executes custom functions.
- **File Handling**: Supports a range of file formats.

## Assistants API Beta
- The Assistants API allows integration of AI assistants into applications.
- Supports tools like Code Interpreter, Retrieval, and Function calling.
- Use the Assistants playground for a code-free exploration.

### Integration Steps
1. **Create an Assistant**: Define instructions, pick a model, and enable tools.
   ```sh
   curl "https://your-domain/v1/assistants" \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer $OPENAI_API_KEY" \
     -H "OpenAI-Beta: assistants=v1" \
     -d '{
       "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
       "name": "Math Tutor",
       "tools": [{"type": "code_interpreter"}],
       "model": "mistralai/Mistral-7B-v0.1"
     }'
   ```
2. **Create a Thread**: Represents a conversation session with user-specific context.
   ```sh
   curl https://your-domain/v1/threads \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer $OPENAI_API_KEY" \
     -H "OpenAI-Beta: assistants=v1" \
     -d ''
   ```
3. **Add Messages**: Include text or files in the Thread.
   ```sh
   curl https://your-domain/v1/threads/thread_abc123/messages \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer $OPENAI_API_KEY" \
     -H "OpenAI-Beta: assistants=v1" \
     -d '{
         "role": "user",
         "content": "I need to solve the equation `3x + 11 = 14`. Can you help me?"
       }'
   ```
4. **Run the Assistant**: Use the Assistant to trigger responses.
   ```sh
   curl https://your-domain/v1/threads/thread_abc123/runs \
     -H "Authorization: Bearer $OPENAI_API_KEY" \
     -H "Content-Type: application/json" \
     -H "OpenAI-Beta: assistants=v1" \
     -d '{
       "assistant_id": "asst_abc123",
       "instructions": "Please address the user as Jane Doe. The user has a premium account."
     }'
   ```

## Tools

### Code Interpreter
- **Functionality**: Executes Python code, processes files, and generates output files (e.g., images, CSVs).
- **Usage**: Enabled by passing `code_interpreter` in the tools parameter.
- **File Processing**: Can parse data from uploaded files, useful for large data volumes or user-uploaded files.
- **Output Handling

**: Generates image files and data files, which can be downloaded using the file ID in the Assistant Message response.

### Knowledge Retrieval
- **Purpose**: Augments the Assistant with external knowledge from uploaded documents.
- **Enabling Retrieval**: Add `retrieval` in the tools parameter of the Assistant.
- **Techniques**: Uses file content in prompts for short documents or performs a vector search for longer documents.
- **File Formats**: Supports a variety of formats including .pdf, .md, .docx, etc.

### Function Calling
- **Capabilities**: Describe functions to Assistants, which intelligently return functions to be called along with arguments.
- **Defining Functions**: Define functions when creating an Assistant.
- **Function Invocation**: The Assistant API pauses execution during a Run when it invokes functions.
- **Output Submission**: Submit tool output from the function calls to continue the Run execution.

## What are people building

- 🚀 Open Source Assistants is being tested at **NASA facilities** to be deployed in space soon.

## Security and Data Access
- Implement strict authorization checks.
- Limit API access within your organization.

## Limitations and Future Developments
- Currently in beta with ongoing developments.
- Future plans include streaming output, notifications, image generation, and image handling in user messages.

---

# Supported File Formats for Tools
| File Format | MIME Type | Code Interpreter | Retrieval |
|-------------|-----------|-------------------|-----------|
| .c          | text/x-c  | ✓                 |           |
| .cpp        | text/x-c++| ✓                 |           |
| .csv        | application/csv | ✓           | ✓         |
| .docx       | application/vnd.openxmlformats-officedocument.wordprocessingml.document | | ✓ |
| .html       | text/html | ✓                 |           |
| ... and many more |

