FROM rust:bookworm

# Set the environment variables
ENV DATABASE_URL postgres://postgres:secret@postgres:5432/mydatabase
ENV REDIS_URL=redis://redis/
ENV S3_ENDPOINT=http://minio1:9000
ENV S3_ACCESS_KEY=minioadmin
ENV S3_SECRET_KEY=minioadmin
ENV S3_BUCKET_NAME=mybucket

# Set the working directory in the image to /app
WORKDIR /app

# Copy the pre-built binaries into the image
COPY ./docker/target/release/hal-9100 /usr/local/bin/hal-9100

# Copy the wait script to the image
COPY ./ee/k8s/readiness-probe.sh /app/readiness-probe.sh
RUN chmod +x /app/readiness-probe.sh

# Copy the configuration file to the image
COPY hal-9100.toml /app/hal-9100.toml

# Copy the entrypoint script
COPY ./docker/entrypoint.sh /app/entrypoint.sh

# Make the entrypoint script executable
RUN chmod +x /app/entrypoint.sh

# Run the entrypoint script when the container launches
ENTRYPOINT ["/app/entrypoint.sh"]