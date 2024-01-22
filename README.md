
<p align="center">
<img width="600" alt="assistants" src="https://github.com/stellar-amenities/assistants/assets/25003283/77d78c9a-cc44-492a-b085-8f22e9d5e4ef">
  <h1 align="center">⭐️ Open Source Edge Assistants API</h1>
  <h2 align="center">Build Powerful AI Assistants In-House, On Your Terms, That run on the Edge</h2>
  <h4 align="center">100% Privacy, 75% Cheaper & 23x Faster Assistants. For the Edge. Same API/SDK.</h4>
  <p align="center">
    <a href='https://codespaces.new/stellar-amenities/assistants?quickstart=1'><img src='https://github.com/codespaces/badge.svg' alt='Open in GitHub Codespaces' style='max-width: 100%;'></a>
    <br />
    <a href="https://discord.gg/pj5VRqqs84"><img alt="Join Discord" src="https://img.shields.io/discord/1066022656845025310?color=blue&style=for-the-badge"></a>
  </p>
</p>


-----

<p align="center">
    <a href="https://link.excalidraw.com/readonly/YSE7DNzB2LmEPfVdCqq3">🖼️ Infra</a>
    <a href="https://github.com/stellar-amenities/assistants/issues/new?assignees=&labels=enhancement">✨ Feature?</a>
    <a href="https://github.com/stellar-amenities/assistants/issues/new?assignees=&labels=bug">❤️‍🩹 Bug?</a>
</p>

-----


# Quickstart

Get started in less than a minute through GitHub Codespaces:

[![Open in GitHub Codespaces](https://github.com/codespaces/badge.svg)](https://codespaces.new/stellar-amenities/assistants?quickstart=1)

Or:

```bash
git clone https://github.com/stellar-amenities/assistants
cd assistants
cp .env.example .env
```

To get started quickly, let's use Perplexity API.
Get an API key from Perplexity. You can get it [here](https://docs.perplexity.ai/docs). Replace in [.env](./.env) the `MODEL_API_KEY` with your API key

Install OpenAI SDK: `npm i openai`

Start the infra:

```bash
docker-compose --profile api -f docker/docker-compose.yml up -d
```

Run the [quickstart](./examples/quickstart.js):

```bash
node examples/quickstart.js
```

## Why Open Source Edge Assistants API?
- **Full Control**: Own your data, your models, and your destiny.
- **No Hidden Costs**: Absolutely free. Seriously, no strings attached.
- **Customizable**: Tailor the AI to your specific needs and use cases.
- **Offline Capabilities**: Perfect for edge cases or internet-free zones.
- **OpenAI Compatibility**: Love OpenAI's API? We play nice with that too.
- **Integration**: Easy to connect to other infrastructures.
- **Non-woke style**: No OpenAI Woke style.

## What's Cooking? – Latest News

- [2024/01/19] 🔥 Action tool. Let your Assistant make requests to APIs.
- [2023/12/19] 🔥 New example: Open source LLM with code interpreter. [Learn more](./examples/hello-world-code-interpreter-mixtral-nodejs/README.md).
- [2023/12/08] 🔥 New example: Open source LLM with function calling. [Learn more](./examples/hello-world-intel-neural-chat-nodejs-function-calling/README.md).
- [2023/11/29] 🔥 New example: Using mistral-7b, an open source LLM. [Check it out](./examples/hello-world-mistral-curl/README.md).

## Key Features
- [x] **Code Interpreter**: Runs Python code in a sandboxed environment. (beta)
- [x] **Knowledge Retrieval**: Retrieves external knowledge or documents.
- [x] **Function Calling**: Defines and executes custom functions.
- [x] **File Handling**: Supports a range of file formats.
- [ ] **Multimodal**: Supports audio, images, and text.
  - [ ] image audio text 
  - [ ] audio text
  - [ ] image text (soon)
  - [x] text

## Join the Movement
- **For Developers**: We've got the docs, tools, and a community ready to help you build what's next.
- **For Innovators**: Looking for an edge in AI? Here's where you leapfrog the competition.
- **For the Visionaries**: Dreamt of a custom AI assistant? Let's make it a reality.

## Deployment

Please follow [this documentation](https://github.com/stellar-amenities/assistants/blob/main/ee/k8s/README.md).

## FAQ

<details>
<summary>Which LLM API can I use?</summary>

Examples of LLM APIs that does not support OpenAI API-like, that you can't use:
- [ollama](https://github.com/stellar-amenities/assistants/issues/24)
- [llama.cpp server example](https://github.com/ggerganov/llama.cpp/tree/master/examples/server)

Examples of LLM APIs that does support OpenAI API-like, that you can use:
- [FastChat (good if you have a mac)](https://github.com/stellar-amenities/assistants/tree/main/examples/hello-world-mistral-curl)
- [vLLM (good if you have a modern gpu)](https://docs.vllm.ai/en/latest/getting_started/quickstart.html#openai-compatible-server)
- [Perplexity API](https://github.com/stellar-amenities/assistants/tree/main/examples/hello-world-code-interpreter-mixtral-nodejs)
- Mistral API
- anyscale
- together ai
</details>

<details>
<summary>What's the difference with LangChain?</summary>
LangChain offers detailed control over AI conversations, while OpenAI's Assistants API simplifies the process, managing conversation history, data/vector store, and tool switching for you.
</details>

<details>
<summary>Are you related to OpenAI?</summary>
No.
</details>

<details>
<summary>I don't use Assistants API. Can I use this?</summary>
We recommend switching to the Assistants API for a more streamlined experience, allowing you to focus more on your product than on infrastructure.
</details>

<details>
<summary>Does the Assistants API support audio and images?</summary>
Images soon, working on it.
Audio in a few weeks.
</details>
