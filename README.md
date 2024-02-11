
<p align="center">
<img width="600" alt="hal-9100" src="https://github.com/stellar-amenities/hal-9100/assets/25003283/f512cf3d-1c24-4d87-85ca-c855c32f5099">
  <h1 align="center">🤖 HAL-9100</h1>
  <h2 align="center">Build Powerful AI Assistants In-House, On Your Terms.</h2>
  <h4 align="center">100% Private, 75% Cheaper & 23x Faster Assistants. Using OpenAI SDK.</h4>
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
    <a href="https://cal.com/louis030195/ai">📞 Help?</a>
</p>

-----


# Quickstart

Since it's compatible with OpenAI Assistants API, this is how you would integrate the client side: 

<img width="600" alt="hal-9100" src="https://github.com/stellar-amenities/assistants/assets/25003283/77d78c9a-cc44-492a-b085-8f22e9d5e4ef">

Get started in less than a minute through GitHub Codespaces:

[![Open in GitHub Codespaces](https://github.com/codespaces/badge.svg)](https://codespaces.new/stellar-amenities/hal-9100?quickstart=1)

Or:

```bash
git clone https://github.com/stellar-amenities/hal-9100
cd hal-9100
cp .env.example .env
```

To get started quickly, let's use Perplexity API.
Get an API key from Perplexity. You can get it [here](https://docs.perplexity.ai/docs). Replace in [.env](./.env) the `MODEL_API_KEY` with your API key

Install OpenAI SDK: `npm i openai`

Start the infra:

```bash
docker compose --profile api -f docker/docker-compose.yml up -d
```

Run the [quickstart](./examples/quickstart.js):

```bash
node examples/quickstart.js
```


## 📏 Principles

<img align="center" width="600" alt="hal-9100" src="https://github.com/stellar-amenities/hal-9100/assets/25003283/225dce60-2bc5-4489-a030-c9d865527a57">
  

HAL-9100 is in continuous development, with the aim of always offering better infrastructure for Edge LLMs. To achieve this, it is based on several principles that define its functionality and scope.

<details>
<summary><strong>Edge-first</strong></summary>
<p>

HAL-9100 does not require internet access by focusing on open source LLMs. Which means you own your data and your models. It runs on a Raspberry PI (LLM included).

</p>
</details>

<details>
<summary><strong>OpenAI-compatible</strong></summary>
<p>

OpenAI spent a large amount of the best brain power to design this API, which makes it an incredible experience for developers.

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



## ⭐️ Latest News

- [2024/01/19] 🔥 Action tool. Let your Assistant make requests to APIs.
- [2023/12/19] 🔥 New example: Open source LLM with code interpreter. [Learn more](./examples/hello-world-code-interpreter-mixtral-nodejs/README.md).
- [2023/12/08] 🔥 New example: Open source LLM with function calling. [Learn more](./examples/hello-world-intel-neural-chat-nodejs-function-calling/README.md).
- [2023/11/29] 🔥 New example: Using mistral-7b, an open source LLM. [Check it out](./examples/hello-world-mistral-curl/README.md).

## ✨ Key Features
- [x] **Code Interpreter**: Runs Python code in a sandboxed environment. (beta)
- [x] **Knowledge Retrieval**: Retrieves external knowledge or documents.
- [x] **Function Calling**: Defines and executes custom functions.
- [x] **Actions**: Execute requests to external APIs, automatically.
- [x] **File Handling**: Supports a range of file formats.

## 😃 Who is it for?

- You operate on a large scale and want to reduce your costs
- You want to increase your speed
- You want to increase customization (e.g. use your own models, extend the API, etc.)
- You work in a data-sensitive environment (healthcare, home AI assistant, military, law, etc.)
- Your product does have poor internet access (military, extreme environment, etc.)

## 🚀 Deployment

Please follow [this documentation](https://github.com/stellar-amenities/hal-9100/blob/main/ee/k8s/README.md).

## 🤔 FAQ

<details>
<summary>Which LLM API can I use?</summary>

Examples of LLM APIs that does not support OpenAI API-like, that you can't use:
- [ollama](https://github.com/stellar-amenities/hal-9100/issues/24)
- [llama.cpp server example](https://github.com/ggerganov/llama.cpp/tree/master/examples/server)

Examples of LLM APIs that does support OpenAI API-like, that you can use:
- [MLC-LLM](https://github.com/mlc-ai/mlc-llm)
- [FastChat (good if you have a mac)](https://github.com/stellar-amenities/hal-9100/tree/main/examples/hello-world-mistral-curl)
- [vLLM (good if you have a modern gpu)](https://docs.vllm.ai/en/latest/getting_started/quickstart.html#openai-compatible-server)
- [Perplexity API](https://github.com/stellar-amenities/hal-9100/tree/main/examples/hello-world-code-interpreter-mixtral-nodejs)
- Mistral API
- anyscale
- together ai
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

