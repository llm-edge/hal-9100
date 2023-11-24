.PHONY: help docker clean

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
