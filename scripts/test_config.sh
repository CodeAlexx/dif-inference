#!/usr/bin/env bash
# Exercise the actual shell bridge, not just the Rust JSON resolver.
set -euo pipefail
config_test_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export DIFC_CONFIG="$config_test_root/config/h3-ref2va.json"
source "$config_test_root/scripts/config.sh"
[[ "$(config_get minimax_h3.policy.streamed_keep_mapped_pages)" == false ]]
export DIFC_CONFIG="$config_test_root/config/difc.json"
source "$config_test_root/scripts/config.sh"
[[ "$(config_get minimax_h3.policy.streamed_keep_mapped_pages)" == true ]]
printf '%s\n' 'PASS shell config preserves false and true across task profiles'
