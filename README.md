

### vision 

Full-stack LLM platform for developing, collaborating, testing, deploying LLM applications in environment without internet access with 1-4 AI accelerators (less than 10^15 FLOPS) on any device.

### status

not usable unless you invest 1h+ to understand

### in progress 

- **runs everywhere**: make it possible to use 100% local storage instead of Redis, Postgres, Minio

### todos

- machine learning compilation: mlc-llm
- wasm compilation + TS example / abstraction
- etc.


### dev principles (wip)

fuck internet (100% disconnected, edge focused)

fuck retrieval

fuck embeddings

yes function calling 

yes API calling 

yes machine learning compilation 

yes runs everywhere (server, client, ios, android, web, etc.) on any AI accelerator

yes wasm

yes any data store

yes dashboard

yes Rust + TS + anything

### dev anti principles (wip)

- langchain is good example of anti pattern (feature creepiness, localhost focused, integrated with tons of internet-only startups). also their new product is proprietary (langsmith)



