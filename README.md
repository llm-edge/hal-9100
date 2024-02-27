
<p align="center">
<img width="600" alt="hal-9100" src="https://github.com/stellar-amenities/hal-9100/assets/25003283/375a1e31-cf39-4592-8f5c-b37df119ebd7">
  <h1 align="center">🤖 HAL-9100</h1>
  <h2 align="center">Build Powerful AI Assistants In-House, On Your Terms. Using OpenAI SDK. For production.</h2>
  <h4 align="center">100% Private, 75% Cheaper & 23x Faster Assistants.</h4>
  <p align="center">
    <a href='https://codespaces.new/stellar-amenities/hal-9100?quickstart=1'><img src='https://github.com/codespaces/badge.svg' alt='Open in GitHub Codespaces' style='max-width: 100%;'></a>
    <br />
    <a href="https://discord.gg/pj5VRqqs84"><img alt="Join Discord" src="https://img.shields.io/discord/1066022656845025310?color=blue&style=for-the-badge"></a>
  </p>


</p>


-----

<p align="center">
    <a href="https://link.excalidraw.com/readonly/YSE7DNzB2LmEPfVdCqq3">🖼️ Infra</a>
    <a href="https://github.com/stellar-amenities/hal-9100/issues/new?assignees=&labels=enhancement">✨ Feature?</a>
    <a href="https://github.com/stellar-amenities/hal-9100/issues/new?assignees=&labels=bug">❤️‍🩹 Bug?</a>
    <a href="https://cal.com/louis030195/applied-ai">📞 Help?</a>
</p>

-----


# ⭐️ Latest News

- [2024/01/19] 🔥 Action tool. [Let your Assistant make requests to APIs](https://github.com/stellar-amenities/hal-9100/tree/main/examples/hello-world-mlc-llm-mistral-nodejs-action).
- [2023/12/19] 🔥 New example: Open source LLM with code interpreter. [Learn more](./examples/hello-world-code-interpreter-mixtral-nodejs/README.md).
- [2023/12/08] 🔥 New example: Open source LLM with function calling. [Learn more](./examples/hello-world-intel-neural-chat-nodejs-function-calling/README.md).
- [2023/11/29] 🔥 New example: Using mistral-7b, an open source LLM. [Check it out](./examples/hello-world-mistral-curl/README.md).

# ✨ Key Features
- [x] **Code Interpreter**: Generate and runs Python code in a sandboxed environment autonomously. (beta)
- [x] **Knowledge Retrieval**: Retrieves external knowledge or documents autonomously.
- [x] **Function Calling**: Defines and executes custom functions autonomously.
- [x] **Actions**: Execute requests to external APIs autonomously.
- [x] **Files**: Supports a range of file formats.
- [x] **Enterprise production-ready**: 
  - [x] observability (metrics, errors, traces, logs, etc.)
  - [x] scalability (serverless, caching, autoscaling, etc.)
  - [x] security (encryption, authentication, authorization, etc.)

# 😃 Who is it for?

- You operate on a large scale and want to reduce your costs
- You want to increase your speed
- You want to increase customization (e.g. use your own models, extend the API, etc.)
- You work in a data-sensitive environment (healthcare, IoT, military, law, etc.)
- Your product does have poor internet access (military, IoT, extreme environment, etc.)

# 📏 Principles

<img align="center" width="600" alt="hal-9100" src="https://github.com/stellar-amenities/hal-9100/assets/25003283/225dce60-2bc5-4489-a030-c9d865527a57">
  

HAL-9100 is in continuous development, with the aim of always offering better infrastructure for Edge LLMs. To achieve this, it is based on several principles that define its functionality and scope.

<details>
<summary><strong>Less prompt is more</strong></summary>
<p>

As few prompts as possible should be hard-coded into the infrastructure, just enough to bridge the gap between **Software 1.0** and **Software 3.0** and give the client as much control as possible on the prompts.

</p>
</details>

<details>
<summary><strong>Edge-first</strong></summary>
<p>

HAL-9100 does not require internet access by focusing on **open source LLMs**. Which means you own your data and your models. It runs on a Raspberry PI (LLM included).

</p>
</details>

<details>
<summary><strong>OpenAI-compatible</strong></summary>
<p>

OpenAI spent a large amount of the best brain power to design this API, which makes it an incredible experience for developers. Support for OpenAI LLMs are not a priority at all though.

</p>
</details>

<details>
<summary><strong>Reliable and deterministic</strong></summary>
<p>

HAL-9100 focus on reliability and being as deterministic as possible by default. That's why everything has to be tested and benchmarked.

</p>
</details>

<details>
<summary><strong>Flexible</strong></summary>
<p>

A minimal number of hard-coded prompts and behaviors, a wide range of models, infrastructure components and deployment options and it play well with the open-source ecosystem, while only integrating projects that have stood the test of time.

</p>
</details>

# Quickstart

Since it's compatible with OpenAI Assistants API, this is how you would integrate the client side: 

<img width="600" alt="hal-9100" src="https://github.com/stellar-amenities/assistants/assets/25003283/77d78c9a-cc44-492a-b085-8f22e9d5e4ef">

Get started in less than a minute through GitHub Codespaces:

[![Open in GitHub Codespaces](https://github.com/codespaces/badge.svg)](https://codespaces.new/stellar-amenities/hal-9100?quickstart=1)

Or:

```bash
git clone https://github.com/stellar-amenities/hal-9100
cd hal-9100
```

To get started quickly, let's use Anyscale API.
Get an API key from Anyscale. You can get it [here](https://app.endpoints.anyscale.com/credentials). Replace in [hal-9100.toml](./hal-9100.toml) the `model_api_key` with your API key.

<details>
<summary>Usage w/ ollama</summary>
<p>

1. use `model_url = "http://ollama:11434/v1/chat/completions"`
2. set `gemma:2b` in [examples/quickstart.js](./examples/quickstart.js)
3. and run `docker compose --profile api --profile ollama -f docker/docker-compose.yml up`

</p>
</details>


Install OpenAI SDK: `npm i openai`

Start the infra:

```bash
docker compose --profile api -f docker/docker-compose.yml up
```

Run the [quickstart](./examples/quickstart.js):

```bash
node examples/quickstart.js
```

# 🤔 FAQ

<details>
<summary>Is there a hosted version?</summary>

No. HAL-9100 is not a hosted service. It's a software that you can deploy on your infrastructure. We can help you deploy it on your infrastructure. [Contact us](https://cal.com/louis030195/applied-ai).
</details>

<details>
<summary>Which LLM API can I use?</summary>


Examples of LLM APIs that does support OpenAI API-like, that you can use:
- ollama
- [MLC-LLM](https://github.com/mlc-ai/mlc-llm)
- [FastChat (good if you have a mac)](https://github.com/stellar-amenities/hal-9100/tree/main/examples/hello-world-mistral-curl)
- [vLLM (good if you have a modern gpu)](https://docs.vllm.ai/en/latest/getting_started/quickstart.html#openai-compatible-server)
- [Perplexity API](https://github.com/stellar-amenities/hal-9100/tree/main/examples/hello-world-code-interpreter-mixtral-nodejs)
- Mistral API
- anyscale
- together ai

We recommend these models:
- mistralai/Mixtral-8x7B-Instruct-v0.1
- mistralai/mistral-7b

Other models have not been extensively tested and may not work as expected, but you can try them.
</details>

<details>
<summary>What's the difference with LangChain?</summary>
You can write AI products in 50 lines of code instead of 5000, and avoid being dependent on a feature-creep project.
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
</details>

