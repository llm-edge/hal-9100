# Use an official Rust runtime as a parent image
FROM rust:1.74 as builder

# Set the working directory in the image to /app
WORKDIR /app

# Copy the current directory contents into the container at /app
COPY . /app

# https://github.com/launchbadge/sqlx/issues/1737
# ENV SQLX_OFFLINE true
# ENV DATABASE_URL postgres://postgres:secret@pg:5432/mydatabase

# Build the consumer and server applications
# RUN cargo build --release --bin run_consumer
# RUN cargo build --release --bin assistants-api-communication

# Start a new stage to create a lean final image
FROM debian:buster-slim

# Set the environment variables
ENV DATABASE_URL postgres://postgres:secret@pg:5432/mydatabase
ENV REDIS_URL=redis://redis/
ENV S3_ENDPOINT=http://minio1:9000
ENV S3_ACCESS_KEY=minioadmin
ENV S3_SECRET_KEY=minioadmin
ENV S3_BUCKET_NAME=mybucket

# Set the working directory
WORKDIR /app

# Copy the binary from the builder stage
# COPY --from=builder /app/target/release/run_consumer /app/run_consumer
# COPY --from=builder /app/target/release/assistants-api-communication /app/assistants-api-communication

# ! HACK tmp cuz of this shit https://github.com/launchbadge/sqlx/issues/1737
# ! https://discord.com/channels/665528275556106240/665528275556106243/1179864937863053372
COPY ./target/release/run_consumer /app/run_consumer
COPY ./target/release/assistants-api-communication /app/assistants-api-communication

# Copy the entrypoint script
COPY docker/entrypoint.sh /app/entrypoint.sh

# Make the entrypoint script executable
RUN chmod +x /app/entrypoint.sh

# Run the entrypoint script when the container launches
ENTRYPOINT ["/app/entrypoint.sh"]
