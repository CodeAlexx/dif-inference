#!/usr/bin/env bash
# Register the entries in the selected JSON; never replace existing user files.
set -euo pipefail
source "$(cd "$(dirname "$0")" && pwd)/config.sh"
exec "$DIFC_CONFIG_TOOL" register
