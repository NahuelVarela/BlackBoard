#!/usr/bin/env bash
# Genuine e2e for plan #1: 2 agents on 2 slices concurrently, each publishing
# via bb verbs; board+TUI show [~]/[x] correctly. No git on the read path.
set -euo pipefail
BB="${BB:-./target/debug/bb}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "== setup =="
cp problems/001-blackboard-cli.md "$TMP/001-blackboard-cli.md"
"$BB" --repo "$TMP" init
"$BB" --repo "$TMP" sync "$TMP/001-blackboard-cli.md" --offline
echo "--- board after sync (all open) ---"
"$BB" --repo "$TMP" board
"$BB" --repo "$TMP" board | grep -q "\[ \] core" || { echo "FAIL: core not open"; exit 1; }

echo "== two agents concurrently =="
# agent-1 takes core, agent-2 takes cli; each publishes planning -> executing -> done (one done each)
(
  "$BB" --repo "$TMP" pick '#1/core' --by agent-1
  sleep 1
  "$BB" --repo "$TMP" state '#1/core' executing --by agent-1 --summary "JSONL segments per writer, schema validated"
  sleep 1
  "$BB" --repo "$TMP" done '#1/core' --by agent-1 --summary "JSONL log plus rusqlite indexer done, verify with bb board."
) &
P1=$!
(
  "$BB" --repo "$TMP" pick '#1/cli' --by agent-2
  sleep 1
  "$BB" --repo "$TMP" done '#1/cli' --by agent-2 --summary "clap verbs wired, verify with bb show #1."
) &
P2=$!
wait $P1 $P2

echo "--- board after agents ---"
"$BB" --repo "$TMP" board
"$BB" --repo "$TMP" board | grep -q "\[x\] *core *done" || { echo "FAIL: core not done"; exit 1; }
"$BB" --repo "$TMP" board | grep -q "\[x\] *cli *done" || { echo "FAIL: cli not done"; exit 1; }
"$BB" --repo "$TMP" board | grep -q "\[ \] *board *open" || { echo "FAIL: board slice changed unexpectedly"; exit 1; }

echo "== token note: bb show line count =="
LINES=$("$BB" --repo "$TMP" show '#1' | wc -l)
echo "bb show #1 lines: $LINES"
[ "$LINES" -le 40 ] || { echo "FAIL: show > 40 lines"; exit 1; }

echo "== tui --once matches board =="
"$BB" --repo "$TMP" tui --once > "$TMP/tui.txt"
"$BB" --repo "$TMP" board > "$TMP/board.txt"
# tui --once prints exactly the board text; board.txt has no legend? both share projection — compare core lines
grep -q "#1" "$TMP/tui.txt" || { echo "FAIL: tui empty"; exit 1; }
diff <(grep -E "\[.\]|\[ \]|\[x\]|\[~\]|\[!\]" "$TMP/board.txt" | sort) \
     <(grep -E "\[.\]|\[ \]|\[x\]|\[~\]|\[!\]" "$TMP/tui.txt" | sort) \
  || { echo "FAIL: tui/board tick mismatch"; exit 1; }

echo "== no git on read path =="
if grep -rn "Command.*git\|process::Command.*\"git\"\|\"git\"" src/ ; then echo "FAIL: git invocation in src/"; exit 1; fi
"$BB" --repo "$TMP" board --explain | grep -q "git=none" || { echo "FAIL: --explain missing"; exit 1; }

echo "E2E OK"
