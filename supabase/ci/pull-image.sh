#!/usr/bin/env bash
set -euo pipefail

image="$1"
delay=5
for attempt in 1 2 3 4 5; do
  if docker pull "$image"; then
    exit 0
  fi
  if (( attempt == 5 )); then
    printf 'Failed to pull %s after %s attempts\n' "$image" "$attempt" >&2
    exit 1
  fi
  printf 'Retrying pull of %s in %s seconds (attempt %s/5)\n' "$image" "$delay" "$((attempt + 1))" >&2
  sleep "$delay"
  delay=$((delay * 2))
done
