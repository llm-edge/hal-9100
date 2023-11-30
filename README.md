<p align="center">
  <a href="mailto:hi@louis030195.com?subject=Interest in Open Source Assistants API">
    <button style="font-size:20px; color:white; background-color:#007BFF; border:none; padding:10px 20px; border-radius:5px; cursor:pointer;">
      🚀 Interested in this API in your infra? Email Me 🚀
    </button>
  </a>
</p>

<p align="center">
<img width="600" alt="assistants" src="https://github.com/stellar-amenities/assistants/assets/25003283/08d69b68-10a3-4ef5-9f99-bd3bc9bbb568">
  <h1 align="center">⭐️ Open Source ⭐️ <s>OpenAI</s> Assistants API</h1>

  <h3 align="center">The ⭐️ Open Source ⭐️ <s>OpenAI</s> Assistants API allows you to build AI assistants within your own applications</h3>

  <p align="center">
    <br />
    <a href="https://discord.gg/XMetBW3zCG"><img alt="Discord" src="https://img.shields.io/discord/1066022656845025310?color=black&style=for-the-badge"></a>
    <br />
    <div align="center">
      <a href="https://link.excalidraw.com/readonly/YSE7DNzB2LmEPfVdCqq3">🖼️ How does it work?</a>
      <br />
      <a href="https://github.com/stellar-amenities/assistants/issues/new?assignees=&labels=enhancement">✨ Request Feature</a>
      <br />
      <a href="https://github.com/stellar-amenities/assistants/issues/new?assignees=&labels=bug">❤️‍🩹 Report Bug</a>
    </div>
    <br />
  </p>
</p>

# Open Source Assistants API

**🚀 Status: experimental - moving fast** DM if you need special [support](mailto:hi@louis030195.com)

[👉 Try it now on your computer](./examples/hello-world-anthropic-curl/README.md)

## Overview
The Open Source Assistants API enables building AI assistants within applications using **Open Source** models or **other AI providers than OpenAI**, tools, and knowledge to respond to user queries. This API is in beta, with continuous enhancements and support for various tools.

### Key Features
- [ ] **Code Interpreter**: Runs Python code in a sandboxed environment.
- [x] **Knowledge Retrieval**: Retrieves external knowledge or documents.
  - [x] Anthropic
  - [x] Open source LLMs
  - [x] OpenAI
- [ ] **Function Calling**: Defines and executes custom functions.
- [x] **File Handling**: Supports a range of file formats.
  - [x] pdf
  - [x] text files
  - [ ] images, videos, etc.

## Assistants API Beta
- The Assistants API allows integration of AI assistants into applications.
- Supports tools like Code Interpreter, Retrieval, and Function calling.
- Will follow OpenAI Assistants evolutions

## Goals 
- **Highly reliable**: The API is designed to be highly reliable, tested, and used in production even in extreme environments with human lives at stake.
- **Compatible with OpenAI Assistants API**: The API is designed to be compatible with OpenAI Assistants API.
- **Thin abstraction layer**: The API is designed as a thin, easy to understand, covering most valuable use cases, layer of abstraction on top of the best Open Source projects that have stood the test of time.
- **Free**: The API is designed to be free and Open Source, with no hidden costs.

### Integration Steps

Assistants follow the same usage than [OpenAI Assistants API](https://platform.openai.com/docs/assistants/overview), the only difference is:

- **Change the API domain:** from api.openai.com to [your-domain] - for example if you deploy Assistants on Railway.app it could be: assistants-aa2d.up.railway.app
- **Remove unnecessary headers:** "Authorization: Bearer xxx" and "OpenAI-Beta: assistants=v1"
- **Set your model:** In some endpoints, you need to set "model" properties, e.g. "mistralai/Mistral-7B-v0.1" (if you're running this model in your infrastructure, whose URL you've configured in the Assistants configuration).

For example, to create an Assistant: 

![Pika UI mockup](https://github.com/stellar-amenities/assistants/assets/25003283/08d69b68-10a3-4ef5-9f99-bd3bc9bbb568)

## System Architecture

The Open Source Assistants API is built on a robust and flexible infrastructure designed to handle a variety of AI tasks. Here's a high-level overview of how it works:

1. **AI Models**: The core of the system is powered by AI models. These models can be from Open Source or other AI providers than OpenAI. You have the flexibility to set your model according to your needs.

2. **API Gateway**: The API Gateway is the entry point for all requests. It routes requests to the appropriate service and handles tasks like rate limiting and authentication.

3. **Services**: The system is composed of several services, each responsible for a specific task. These include a Code Interpreter, Knowledge Retrieval, Function Calling, and File Handling.

4. **Data Storage**: The system uses a combination of in-memory and persistent storage to manage data. This ensures fast response times and durability.

5. **Deployment**: The system can be deployed on any platform that supports Docker, such as Railway.app.