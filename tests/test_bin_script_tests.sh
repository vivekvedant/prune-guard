#!/usr/bin/env bash
set -euo pipefail

SCRIPT_PATH="${1:-test_bin.sh}"

if [[ ! -f "$SCRIPT_PATH" ]]; then
  echo "missing script: $SCRIPT_PATH"
  exit 1
fi

assert_contains() {
  local pattern="$1"
  local message="$2"
  if ! rg -n "$pattern" "$SCRIPT_PATH" >/dev/null 2>&1; then
    echo "FAIL: $message"
    exit 1
  fi
}

assert_contains '^#!/usr/bin/env bash' "script must have bash shebang"
assert_contains '^set -euo pipefail' "script must fail closed with strict shell options"
assert_contains 'PG_STRESS_MAX_ITERATIONS' "script must support max-iteration guard"
assert_contains 'while :|while true' "script must loop to keep increasing storage"
assert_contains 'docker build' "script must build images to grow image storage"
assert_contains 'docker volume create' "script must create volumes to grow volume storage"
assert_contains 'docker create --name' "script must create stopped containers to grow container metadata"

echo "PASS: test_bin.sh storage growth contract"
