#!/usr/bin/env bash
# Source once, before changing directory. No eval or shell interpolation of JSON.
DIFC_REPO="${SERENITY_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
export SERENITY_REPO_ROOT="$DIFC_REPO"
export DIFC_CONFIG="${DIFC_CONFIG:-$DIFC_REPO/config/difc.json}"
export DIFC_CONFIG="$(realpath -e "$DIFC_CONFIG")" || return 1
# This is the config bootstrap executable, not a model/machine setting.
DIFC_CONFIG_TOOL="$DIFC_REPO/serenity-server/target/release/difc-config"
[[ -x "$DIFC_CONFIG_TOOL" ]] || { echo 'Run scripts/build.sh to build the native config reader first.' >&2; return 1; }
DIFC_DOCUMENT="$("$DIFC_CONFIG_TOOL" resolve)" || return 1
# Flatten once. Reading each setting must not start another JSON-reader process
# in the prompt-to-file chain. NUL framing preserves spaces, quotes and newlines.
declare -A DIFC_VALUES=()
while IFS= read -r -d '' config_key && IFS= read -r -d '' config_value; do
  DIFC_VALUES["$config_key"]="$config_value"
done < <(jq -jr 'paths(type != "array" and type != "object") as $p | ($p | map(tostring) | join(".")), "\u0000", (getpath($p) | tostring), "\u0000"' <<<"$DIFC_DOCUMENT")
config_get() {
  [[ -v "DIFC_VALUES[$1]" ]] || { echo "Missing config key: $1" >&2; return 1; }
  printf '%s\n' "${DIFC_VALUES[$1]}"
}
