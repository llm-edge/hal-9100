#!/bin/bash

# Run the executor and server applications concurrently
# Options:
#   -c, --config <config>  Read configuration from one file. File format is detected from the file name. If zero file are specified, the deprecated default config path `/etc/hal-9100/hal-9100.toml` is targeted [env: HAL_9100_CONFIG=]
#   -v, --verbose...       Enable more detailed internal logging. Repeat to increase level. Overridden by `--quiet`
#   -q, --quiet...         Reduce detail of internal logging. Repeat to reduce further. Overrides `--verbose`
#   -w, --watch-config     Watch for changes in configuration file, and reload accordingly [env: HAL_9100_WATCH_CONFIG=]
#   -p, --port <PORT>      Port to listen on [env: PORT=] [default: 3000]
#   -h, --help             Print help
#   -V, --version          Print version
# e.g. -c ./config.toml -v -p 3000
hal-9100 "$@" executor &
hal-9100 "$@" api &

# Wait for all background processes to finish
wait
