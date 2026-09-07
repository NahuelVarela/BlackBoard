#!/usr/bin/env bash
# Reads demo/data/summary.json (shape: demo/backend/SCHEMA.md) and prints an
# ASCII dashboard to stdout.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="$SCRIPT_DIR/../data"
IN_FILE="$DATA_DIR/summary.json"

if [[ ! -s "$IN_FILE" ]]; then
  echo "error: $IN_FILE missing or empty — run demo/backend/aggregate.sh first" >&2
  exit 1
fi

jq -r '
  def pad(width): (. + (" " * width))[0:width];
  def bar(n; max; width):
    (if max > 0 then (((n * width) / max) | floor) else 0 end) as $len
    | (if $len > 0 then ("#" * $len) else "" end) | pad(width);

  . as $s
  | ($s.revenue_cents / 100 | floor) as $dollars
  | ($s.revenue_cents % 100) as $cents
  | ($cents | tostring | if length < 2 then "0" + . else . end) as $cents2
  | ($s.by_event_type | to_entries) as $events
  | (($events | map(.value) | max) // 0) as $emax
  | ($s.top_pages) as $pages
  | (($pages | map(.count) | max) // 0) as $pmax
  | [
      "=== Analytics Dashboard ===",
      "Generated: \($s.generated_at)",
      "",
      "Total events:  \($s.total_events)",
      "Unique users:  \($s.unique_users)",
      "Revenue:       $\($dollars).\($cents2)",
      "",
      "Events by type:"
    ]
    + ($events | map("  " + (.key | pad(12)) + " [" + bar(.value; $emax; 20) + "] " + (.value | tostring)))
    + ["", "Top pages:"]
    + ($pages | map("  " + (.page | pad(12)) + " [" + bar(.count; $pmax; 20) + "] " + (.count | tostring)))
    | .[]
' "$IN_FILE"
