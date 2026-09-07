#!/usr/bin/env bash
# Reads demo/data/events.jsonl (schema: demo/analytics/SCHEMA.md) and writes
# demo/data/summary.json — see demo/backend/SCHEMA.md for the shape this
# script commits to.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="$SCRIPT_DIR/../data"
IN_FILE="$DATA_DIR/events.jsonl"
OUT_FILE="$DATA_DIR/summary.json"

if [[ ! -s "$IN_FILE" ]]; then
  echo "error: $IN_FILE missing or empty — run demo/analytics/gen_events.sh first" >&2
  exit 1
fi

jq -s '
  {
    total_events: length,
    unique_users: (map(.user_id) | unique | length),
    revenue_cents: (map(.value_cents) | add // 0),
    by_event_type: (
      reduce .[] as $e
        ({page_view: 0, click: 0, signup: 0, purchase: 0}; .[$e.event_type] += 1)
    ),
    top_pages: (
      group_by(.page)
      | map({page: .[0].page, count: length})
      | sort_by(-.count)
      | .[0:5]
    ),
    generated_at: (now | todateiso8601)
  }
' "$IN_FILE" >"$OUT_FILE"

echo "wrote summary to $OUT_FILE"
