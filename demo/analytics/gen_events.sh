#!/usr/bin/env bash
# Generates demo/data/events.jsonl — see demo/analytics/SCHEMA.md for the
# field names and event_type values this script commits to.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="$SCRIPT_DIR/../data"
OUT_FILE="$OUT_DIR/events.jsonl"

mkdir -p "$OUT_DIR"
: > "$OUT_FILE"

EVENT_TYPES=(page_view click signup purchase)
PAGES=(/home /pricing /docs /blog /checkout /signup /about)
NUM_EVENTS=40
NUM_USERS=8

start_epoch=$(date -u +%s)

for ((i = 1; i <= NUM_EVENTS; i++)); do
  event_type="${EVENT_TYPES[$((RANDOM % ${#EVENT_TYPES[@]}))]}"
  page="${PAGES[$((RANDOM % ${#PAGES[@]}))]}"
  user_id=$(printf "user-%03d" "$((RANDOM % NUM_USERS + 1))")
  event_id=$(printf "evt-%04d" "$i")
  timestamp=$(date -u -d "@$((start_epoch + i * 17))" +"%Y-%m-%dT%H:%M:%SZ")

  if [[ "$event_type" == "purchase" ]]; then
    value_cents=$(((RANDOM % 9000) + 500))
  else
    value_cents=0
  fi

  jq -nc \
    --arg event_id "$event_id" \
    --arg event_type "$event_type" \
    --arg timestamp "$timestamp" \
    --arg user_id "$user_id" \
    --arg page "$page" \
    --argjson value_cents "$value_cents" \
    '{event_id: $event_id, event_type: $event_type, timestamp: $timestamp, user_id: $user_id, page: $page, value_cents: $value_cents}' \
    >>"$OUT_FILE"
done

echo "wrote $NUM_EVENTS events to $OUT_FILE"
