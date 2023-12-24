FROM rust:bookworm as builder

# Set the working directory in the image to /app
WORKDIR /app

# Copy the current directory contents into the container at /app
COPY assistants-api-communication /app/assistants-api-communication
COPY assistants-core /app/assistants-core
COPY assistants-extra /app/assistants-extra
COPY Cargo.toml /app/Cargo.toml
COPY .sqlx /app/.sqlx

ENV SQLX_OFFLINE true

RUN cargo build --release --bin run_consumer && \
    cargo build --release --bin assistants-api-communication

# Start a new stage to create a lean final image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl-dev openssl curl

# Set the environment variables
ENV DATABASE_URL postgres://postgres:secret@postgres:5432/mydatabase
ENV REDIS_URL=redis://redis/
ENV S3_ENDPOINT=http://minio1:9000
ENV S3_ACCESS_KEY=minioadmin
ENV S3_SECRET_KEY=minioadmin
ENV S3_BUCKET_NAME=mybucket

# Set the working directory
WORKDIR /app

# Copy the wait script to the image
COPY ./ee/k8s/readiness-probe.sh /app/readiness-probe.sh
RUN chmod +x /app/readiness-probe.sh

# Copy the binary from the builder stage
COPY --from=builder /app/target/release/run_consumer /usr/local/bin/run_consumer
COPY --from=builder /app/target/release/assistants-api-communication /usr/local/bin/assistants-api-communication

# Copy the entrypoint script
COPY ./docker/entrypoint.sh /app/entrypoint.sh

# Make the entrypoint script executable
RUN chmod +x /app/entrypoint.sh

# Run the entrypoint script when the container launches
ENTRYPOINT ["/app/entrypoint.sh"]