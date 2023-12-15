


https://github.com/stellar-amenities/assistants/assets/25003283/191f1f3d-3a06-4f93-9107-5d89ea9b289e

At the moment, you need both **Docker** installed to run the API.

Additionally, `Assistants` currently supports Anthropic and Open Source LLMs, you need an API key that you can put in a `.env` file in the root of the project:

```bash
ANTHROPIC_API_KEY="..."
DATABASE_URL=postgres://postgres:secret@localhost:5432/mydatabase
REDIS_URL=redis://127.0.0.1/
S3_ENDPOINT=http://localhost:9000
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
S3_BUCKET_NAME=mybucket
```

## Usage 

1. Run `make reboot` from the root of `assistants` to start the database, redis and minio
2. Run `make all` from the root of `assistants` to start the API server
3. Open the file index.html with a browser (double click)
