

<p align="center">
<img width="150" alt="assistants" src="https://github.com/louis030195/assistants/assets/25003283/499b65e0-38fb-464b-a4d3-bb7f83f2a81b">
  <h1 align="center">⭐️ Open Source ⭐️ <s>OpenAI</s> Assistants API</h1>

  <h3 align="center">The ⭐️ Open Source ⭐️ <s>OpenAI</s> Assistants API allows you to build AI assistants within your own applications</h3>

  <p align="center">
    <br />
    <a href="https://discord.gg/XMetBW3zCG"><img alt="Discord" src="https://img.shields.io/discord/1066022656845025310?color=black&style=for-the-badge"></a>
    <br />
    <div align="center">
      <a href="stripelink">💰 Pre-order the commercial license 10x cheaper now</a>
      <br />
      <a href="https://github.com/stellar-amenities/assistants/issues/new?assignees=&labels=enhancement">✨ Request Feature</a>
      <br />
      <a href="https://github.com/stellar-amenities/assistants/issues/new?assignees=&labels=bug">❤️‍🩹 Report Bug</a>
    </div>
    <br />
  </p>
</p>

# Open Source Assistants API

**Code coming soon - cleaning up**

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

Assistants follow the same usage than [OpenAI Assistants API](https://platform.openai.com/docs/assistants/overview), the only difference is:

- **Change the API domain:** from api.openai.com to [your-domain] - for example if you deploy Assistants on Railway.app it could be: assistants-aa2d.up.railway.app
- **Remove unnecessary headers:** "Authorization: Bearer xxx" and "OpenAI-Beta: assistants=v1"
- **Set your model:** In some endpoints, you need to set "model" properties, e.g. "mistralai/Mistral-7B-v0.1" (if you're running this model in your infrastructure, whose URL you've configured in the Assistants configuration).

For example, to create an Assistant: 

```sh
curl "https://your-domain/v1/assistants" \
 -H "Content-Type: application/json" \
 -d '{
   "instructions": "You are a personal math tutor. Write and run code to answer math questions.",
   "name": "Math Tutor",
   "tools": [{"type": "code_interpreter"}],
   "model": "mistralai/Mistral-7B-v0.1"
 }'
```





