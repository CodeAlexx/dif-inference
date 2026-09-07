#!/usr/bin/env bash
# Compatibility entry point: the registry is now entirely JSON-driven.
set -euo pipefail
exec bash "$(cd "$(dirname "$0")" && pwd)/register-image-models.sh" "$@"
