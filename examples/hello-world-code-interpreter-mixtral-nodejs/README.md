# Open Source Assistants API Quickstart Guide

This guide demonstrates how to use the Open Source Assistants API to create an assistant that can answer questions about startup investments using `code_interpreter` and `function` tools and `mixtral` LLM.

**Function calling** is a more precise and automatic way to provide context to an LLM than retrieval.

By default LLMs are bad at complex math. **Code interpreter** is a tool used by the LLM to generate python code and execute it to simplify complex math.

Dataset we will use contains the following:

```csv
Startup,Revenue,CapitalRaised,GrowthRate,FundingRound,Investor
StartupA,500000,1000000,0.2,Series A,InvestorX
StartupB,600000,1500000,0.3,Series B,InvestorY
StartupC,700000,2000000,0.4,Series C,InvestorZ
StartupD,800000,2500000,0.5,Series D,InvestorW
StartupE,900000,3000000,0.6,Series E,InvestorV
```

## Prerequisites

We will use Perplexity API to get started quickly with an LLM but you can run this example with any LLM.

1. Get an API key from Perplexity. You can get it [here](https://docs.perplexity.ai/docs). Replace in [.env](./.env) the `MODEL_API_KEY` with your API key.
2. Install OpenAI SDK: `npm i openai`
3. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

## Setup

1. Start Postgres, Redis, Minio, and the server: `make reboot && make all`. This will take a few seconds.

## Running the example

Run the [example](./examples/quickstart.js) using Node.js: `node ./examples/hello-world-code-interpreter-mixtral-nodejs/quickstart.js`

You should see at the end:

>Based on your specified capital investment range of $10K to $1B, I recommend considering investing in StartupB. It has a revenue of $600,000, capital raised of $1.5M, a growth rate of 0.3, and is in Series B funding round. Investor Y is the investor for this startup.
## What did happen?

In `quickstart.js`, we're creating a VC assistant using the Open Source Assistants API. Here's a step-by-step breakdown:

1. **Setup**: We import the necessary modules and initialize the OpenAI SDK with the local server as base URL.

2. **Get VC Capital**: We define a function `getVCCapital` that returns the capital range the VC is willing to invest.

3. **Upload File**: We upload a CSV file containing startup data to the OpenAI API.

4. **Create Assistant**: We create an assistant with specific instructions and tools. In this case, the assistant is a VC bot that uses a function to get the VC's capital and a code interpreter to analyze startup data.

5. **Create Thread**: We create a new thread for the assistant to operate in.

6. **Create Message**: We create a user message asking which startup to invest in.

7. **Create Run**: We create a run, which is an instance of the assistant performing its task.

8. **Get Run**: We retrieve the run to check its status. If the run requires an action, we submit the VC's capital as the output of the function call.

9. **Submit Tool Outputs**: Once we fetched the VC's capital, we submit the output to the assistant.

10. **Get Messages**: Finally, we retrieve all messages in the thread. This includes the user's original question and the assistant's response. The LLM is able to answer the question by using the precise context provided by the function call and the code interpreter.

This script demonstrates how to use the Open Source Assistants API to create an interactive assistant that can answer questions using function calls and code interpretation.

## What's Next?

Now that you've got your feet wet with the Open Source Assistants API, it's time to dive deeper. Check out the `examples` directory for more complex examples and use-cases. 

For those interested in self-hosting, take a look at the [Self-Hosting Guide](./ee/k8s/README.md) in the `./ee/k8s/` directory. It provides detailed instructions on how to set up and manage your own instance.

If you're looking for inspiration on what to build next, check out the [IDEAS.md](../IDEAS.md) file in the `examples` directory. It contains a list of project ideas that leverage the power of the Open Source Assistants API, ranging from AI-powered personal budgeting apps to language learning apps, health trackers, and more.

You can also explore the OpenAI Examples for a wider range of applications and to understand how to leverage the full power of the API.

Remember, the only limit is your imagination. Happy coding!

## Troubleshooting

If you run into any issues, here's what you can do:
- Restart the infrastructure: `make reboot`
- Restart the API server: `make all`

If you still run into issues, please contact @louis030195 on [Discord](https://discord.gg/XMetBW3zCG).
Or book a call [here](https://cal.com/louis030195/unleash-llms). 
