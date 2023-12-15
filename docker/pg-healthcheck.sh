#!/bin/bash
set -eo pipefail

if PGPASSWORD=secret psql -U postgres -d mydatabase -c "SELECT * FROM runs;" > /dev/null 2>&1; then
  exit 0
else
  exit 1
fi

