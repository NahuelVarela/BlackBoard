#!/usr/bin/env bash
# E2E for plan #2: Open/Closed tabs, CLI parity, empty states. Genuine e2e.
set -euo pipefail
BB="${BB:-./target/debug/bb}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "== setup: sync 001 fixture (all open) =="
cp problems/001-blackboard-cli.md "$TMP/001-blackboard-cli.md"
"$BB" --repo "$TMP" init
"$BB" --repo "$TMP" sync "$TMP/001-blackboard-cli.md" --offline >/dev/null

echo "== all-open: Closed tab shows empty-state =="
"$BB" --repo "$TMP" board --closed | grep -q "(empty — nothing closed yet)" || { echo "FAIL: closed-empty missing"; exit 1; }
"$BB" --repo "$TMP" tui --once --tab closed | grep -q "(empty — nothing closed yet)" || { echo "FAIL: tui closed-empty missing"; exit 1; }

echo "== mixed fixture: 2 done + planning/executing/blocked =="
"$BB" --repo "$TMP" pick '#1/core' --by agent-1 >/dev/null
"$BB" --repo "$TMP" done '#1/core' --by agent-1 --summary "Core done, verify with bb board." >/dev/null
"$BB" --repo "$TMP" pick '#1/cli' --by agent-2 >/dev/null
"$BB" --repo "$TMP" done '#1/cli' --by agent-2 --summary "CLI done, verify with bb show." >/dev/null
"$BB" --repo "$TMP" pick '#1/board' --by agent-1 >/dev/null
"$BB" --repo "$TMP" state '#1/board' executing --by agent-1 --summary "Building indexer" >/dev/null
"$BB" --repo "$TMP" pick '#1/tui' --by agent-2 >/dev/null
# tui stays planning; sync -> blocked
"$BB" --repo "$TMP" pick '#1/sync' --by agent-1 >/dev/null
"$BB" --repo "$TMP" state '#1/sync' blocked --by agent-1 --summary "Waiting on gh token, unblocks when offline flag passed." >/dev/null

echo "--- board --open ---"
"$BB" --repo "$TMP" board --open
"$BB" --repo "$TMP" board --open | grep -q "\[~\] *board *executing" || { echo "FAIL: open missing executing"; exit 1; }
"$BB" --repo "$TMP" board --open | grep -q "\[.\] *tui *planning" || { echo "FAIL: open missing planning"; exit 1; }
"$BB" --repo "$TMP" board --open | grep -q "\[!\] *sync *blocked" || { echo "FAIL: blocked not in open"; exit 1; }
if "$BB" --repo "$TMP" board --open | grep -Eq "^  \[x\]"; then echo "FAIL: done leaked into open"; exit 1; fi

echo "--- board --closed ---"
"$BB" --repo "$TMP" board --closed
"$BB" --repo "$TMP" board --closed | grep -q "\[x\] *core *done" || { echo "FAIL: closed missing core"; exit 1; }
"$BB" --repo "$TMP" board --closed | grep -q "\[x\] *cli *done" || { echo "FAIL: closed missing cli"; exit 1; }
if "$BB" --repo "$TMP" board --closed | grep -Eq "^  \[~\]|^  \[\.\]|^  \[ \]|^  \[!\]"; then echo "FAIL: open leaked into closed"; exit 1; fi

echo "== parity: board --open/--closed == tui --once --tab =="
diff <("$BB" --repo "$TMP" board --open) <("$BB" --repo "$TMP" tui --once --tab open) || { echo "FAIL: open parity"; exit 1; }
diff <("$BB" --repo "$TMP" board --closed) <("$BB" --repo "$TMP" tui --once --tab closed) || { echo "FAIL: closed parity"; exit 1; }
diff <("$BB" --repo "$TMP" board) <("$BB" --repo "$TMP" tui --once) || { echo "FAIL: full parity"; exit 1; }

echo "== full board unchanged shape (5 slices + legend) =="
"$BB" --repo "$TMP" board | grep -q "^\[.\] open\|Legend" || true
LINES=$("$BB" --repo "$TMP" board | wc -l)
# legend(1) + header(1) + 5 slices + refs(1) + 1 next: for the blocked slice = 9
[ "$LINES" -eq 9 ] || { echo "FAIL: full board lines=$LINES want 9"; exit 1; }
# blocked slice carries its actionable next step inline (no guessing)
"$BB" --repo "$TMP" board | grep -q "next: bb answer\|next: bb dispatch" || { echo "FAIL: blocked next: hint missing"; exit 1; }

echo "== all-done: Open tab shows empty-state =="
"$BB" --repo "$TMP" done '#1/board' --by agent-1 --summary "Indexer done, verify with bb board." >/dev/null
"$BB" --repo "$TMP" done '#1/tui' --by agent-2 --summary "TUI done, verify with bb tui --once." >/dev/null
"$BB" --repo "$TMP" done '#1/sync' --by agent-1 --summary "Sync done, verify by re-running bb sync." >/dev/null
"$BB" --repo "$TMP" board --open | grep -q "(empty — everything is done)" || { echo "FAIL: open-empty missing"; exit 1; }
"$BB" --repo "$TMP" tui --once --tab open | grep -q "(empty — everything is done)" || { echo "FAIL: tui open-empty missing"; exit 1; }

echo "== flags mutually exclusive =="
"$BB" --repo "$TMP" board --open --closed 2>/dev/null && { echo "FAIL: --open --closed should conflict"; exit 1; } || true

echo "== no git on read path =="
if grep -rn '"git"' src/ ; then echo "FAIL: git invocation in src/"; exit 1; fi
"$BB" --repo "$TMP" board --explain | grep -q "git=none" || { echo "FAIL: --explain missing"; exit 1; }
"$BB" --repo "$TMP" board --open --explain | grep -q "git=none" || { echo "FAIL: --open --explain missing"; exit 1; }

echo "E2E-TABS OK"
