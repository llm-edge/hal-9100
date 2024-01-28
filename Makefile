SHELL := /bin/bash
RUSTC := $(shell command -v rustc 2> /dev/null)

ifndef RUSTC
  $(error "Rust is not available on your system, please install it using: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh")
endif

.PHONY: help docker clean

include .env
export

.DEFAULT_GOAL := help

## Colors
YELLOW := $(shell tput -Txterm setaf 3)
RESET := $(shell tput -Txterm sgr0)

## Help documentation
help:
	@echo "${YELLOW}Available commands:${RESET}"
	@echo
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

## Docker compose up
docker: ## Run docker compose up
	docker-compose -f docker/docker-compose.yml up -d
	@echo "Waiting for the infra to be ready..."
	@while ! docker exec -it pg pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
	@echo "Database is up and running"

## Stop and remove containers
clean: ## Stop and remove docker-postgres-1, docker-redis-1, docker-minio-1 containers
	docker stop $$(docker ps -a -q --filter name=pg --filter name=redis --filter name=minio1) > /dev/null 2>&1 || echo "No containers to stop"
	docker rm $$(docker ps -a -q --filter name=pg --filter name=redis --filter name=minio1) > /dev/null 2>&1 || true
	docker volume rm $$(docker volume ls -q --filter name=pg --filter name=redis --filter name=minio1) > /dev/null 2>&1 || true

## Clean and run docker compose up
reboot: clean docker ## Clean and run docker compose up

## Run the consumer
consumer: ## Run the consumer
	@source .env && cargo run --package assistants-core --bin run_consumer


## Run the server
server: ## Run the server
	@source .env && cargo run --package assistants-api-communication

## Run consumer, server, and dockers
all:
	@source .env && $(MAKE) -j2 consumer server


## Test all
test: ## Run all tests
	@source .env && RUST_TEST_THREADS=1 cargo test --features ci


##@ Development
## Check db/queue content
check: ## Check db/queue content
	@echo "Here's a one-liner Docker CLI command to display the content of the runs table:"
	@echo "docker exec -it pg psql -U postgres -d mydatabase -c \"SELECT * FROM runs;\""
	@echo "Here's a one-liner Docker CLI command to display the content of your Redis instance:"
	@echo "docker exec -it redis redis-cli LRANGE run_queue 0 -1"

## Build the Docker image for the code interpreter
docker-build-code-interpreter-amd64: ## Build the Docker image for the code interpreter for Linux amd64
	docker build --platform linux/amd64 -f docker/Dockerfile.code-interpreter -t code-interpreter-amd64 .

docker-push-code-interpreter-amd64: ## Push the Docker image for the code interpreter to DockerHub for Linux amd64
	docker tag code-interpreter-amd64:latest louis030195/assistants-code-interpreter:latest
	docker push louis030195/assistants-code-interpreter:latest

clean/rust:
	cargo clean

clean/js:
	rm -rf node_modules package-lock.json package.json

clean/docker:
	docker system prune --volumes


runcoqofrust:

	export LD_LIBRARY_PATH=/home/mdupont/.rustup/toolchains/nightly-2023-12-15-x86_64-unknown-linux-gnu/lib/:$LD_LIBRARY_PATH  cargo coq-of-rust > coq-of-rust.txt 2>&1
