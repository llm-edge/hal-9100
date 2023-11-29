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
	while ! docker exec -it pg pg_isready -U postgres; do sleep 1; done
	docker exec -it pg psql -U postgres -c "CREATE DATABASE mydatabase;" > /dev/null 2>&1 || echo "Database already exists"
	docker exec -i pg psql -U postgres -d mydatabase < assistants-core/src/migrations.sql > /dev/null 2>&1 || echo "Migrations already applied"

## Stop and remove containers
clean: ## Stop and remove docker-postgres-1, docker-redis-1, docker-minio-1 containers
	docker stop $$(docker ps -a -q --filter name=pg --filter name=redis --filter name=minio1) > /dev/null 2>&1 || echo "No containers to stop"
	docker rm $$(docker ps -a -q --filter name=pg --filter name=redis --filter name=minio1) > /dev/null 2>&1 || true 
	docker volume rm $$(docker volume ls -q --filter name=pg --filter name=redis --filter name=minio1) > /dev/null 2>&1 || true

## Clean and run docker compose up
reboot: clean docker ## Clean and run docker compose up

## Run the consumer
consumer: ## Run the consumer
	@set -a && . .env && set +a && \
	cargo run --package assistants-core --bin run_consumer


## Run the server
server: ## Run the server
	@set -a && . .env && set +a && \
	cargo run --package assistants-api-communication

## Run consumer, server, and dockers
all: reboot
	@$(MAKE) -j2 consumer server

display: ## Display the ascii art
	@echo "$$ASCII_ART"

define ASCII_ART
 ________  ________   ________  ___  ________  _________  ________  ________   _________  ________      
|\   __  \|\   ____\ |\   ____\|\  \|\   ____\|\___   ___\\   __  \|\   ___  \|\___   ___\\   ____\     
\ \  \|\  \ \  \___|_\ \  \___|\ \  \ \  \___|\|___ \  \_\ \  \|\  \ \  \\ \  \|___ \  \_\ \  \___|_    
 \ \   __  \ \_____  \\ \_____  \ \  \ \_____  \   \ \  \ \ \   __  \ \  \\ \  \   \ \  \ \ \_____  \   
  \ \  \ \  \|____|\  \\|____|\  \ \  \|____|\  \   \ \  \ \ \  \ \  \ \  \\ \  \   \ \  \ \|____|\  \  
   \ \__\ \__\____\_\  \ ____\_\  \ \__\____\_\  \   \ \__\ \ \__\ \__\ \__\\ \__\   \ \__\  ____\_\  \ 
    \|__|\|__|\_________\\_________\|__|\_________\   \|__|  \|__|\|__|\|__| \|__|    \|__| |\_________\
             \|_________\|_________|   \|_________|                                         \|_________|
                                                                                                        
						___                          ___                          ___     
						/__/\                        /__/\                        /__/\    
						\  \:\                       \  \:\                       \  \:\   
						\__\:\                       \__\:\                       \__\:\  
						/  /::\                      /  /::\                      /  /::\ 
						/  /:/\:\                    /  /:/\:\                    /  /:/\:\
						/  /:/__\/                   /  /:/__\/                   /  /:/__\/
					/__/:/                       /__/:/                       /__/:/     
					\__\/                        \__\/                        \__\/      
																							
endef
export ASCII_ART


## Test all
test: ## Run all tests
	@set -a && . .env && set +a && \
	RUST_TEST_THREADS=1 cargo test --features ci


# RUST_TEST_THREADS=1 cargo test --features ci -- --skip test_end_to_end_with_file_upload_and_retrieval && \
# consumer & echo $$! > consumer.pid && \
# sleep 5 && \
# RUST_TEST_THREADS=1 cargo test --features ci -- test_end_to_end_with_file_upload_and_retrieval && \
# kill `cat consumer.pid` && rm consumer.pid

