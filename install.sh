#!/bin/sh
# Stable compatibility entrypoint for v0.2.0 checkouts and automation.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec /bin/sh "$ROOT/packaging/install.sh" "$@"
