SHELL := /bin/bash
RUSTC := $(shell command -v rustc 2> /dev/null)

ifndef RUSTC
  $(warning "Rust is not available on your system, please install it using: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh")
endif

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

