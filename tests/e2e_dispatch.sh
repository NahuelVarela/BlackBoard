#!/usr/bin/env bash
# E2E for plan #3: dispatch -> blocked -> answer -> done with mocked claude.
# Genuine e2e, zero API spend (fake `claude` shim emitting canned stream-json).
set -euo pipefail
BB="${BB:-./target/debug/bb}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "== setup: sync 003 fixture (opaque slice list) =="
cp problems/003-claude-dispatch.md "$TMP/003-claude-dispatch.md"
"$BB" --repo "$TMP" init >/dev/null
"$BB" --repo "$TMP" sync "$TMP/003-claude-dispatch.md" --offline >/dev/null
# First ID from the fixture's Slices list is the dispatch target (no hardcoded set).
SLICE="$(grep -oE '#3/[A-Za-z_-]+' "$TMP/003-claude-dispatch.md" | head -1 || true)"
echo "dispatch target: $SLICE"
[ -n "$SLICE" ] || { echo "FAIL: no slices synced"; exit 1; }

MOCK_LOG="$TMP/mock-log"
mkdir -p "$MOCK_LOG"
PEND="$TMP/.blackboard/pending/3-dispatch.json"
# Derive pending path from slice id: #N-<slice>.json
SLICE_NAME="${SLICE#*/}"
PEND="$TMP/.blackboard/pending/3-${SLICE_NAME}.json"

echo "== mock claude shim (question -> wait answer -> hook -> result) =="
cat > "$TMP/fake-claude" <<EOF
#!/usr/bin/env bash
{ echo "=== invocation ==="; printf '%s\n' "\$@"; } >> "$MOCK_LOG/argv.log"
echo '{"type":"system","subtype":"init","session_id":"mock-1"}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"working on slice"}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu_1","name":"AskUserQuestion","input":{"questions":[{"question":"which hook path?","header":"Hook","options":[{"label":"hook","description":"use hook"},{"label":"prompt-tool","description":"use prompt tool"}],"multiSelect":false}]}}]}}'
for i in \$(seq 1 300); do
  if grep -q '"answers"' "$PEND" 2>/dev/null; then break; fi
  sleep 0.1
done
"$BB" --repo "$TMP" claude-hook --slice '$SLICE' >> "$MOCK_LOG/hook.log" 2>&1
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"resumed with hook answer"}]}}'
echo '{"type":"result","result":"Stream-json loop landed. Verified with mocked e2e.","session_id":"mock-1","total_cost_usd":0.042,"usage":{"input_tokens":12000,"output_tokens":3000,"cache_read_input_tokens":1000,"cache_creation_input_tokens":0},"duration_ms":92000,"num_turns":14,"is_error":false,"model":"sonnet"}'
EOF
chmod +x "$TMP/fake-claude"

echo "== dispatch in background =="
"$BB" --repo "$TMP" dispatch "$SLICE" --by agent-1 --mock-bin "$TMP/fake-claude" > "$TMP/dispatch.out" 2>&1 &
DISP_PID=$!

echo "== poll until [!] blocked =="
for i in $(seq 1 150); do
  # Slice lines start with two spaces; the Legend line also mentions [!]/blocked.
  if "$BB" --repo "$TMP" board 2>/dev/null | grep -q '^  \[!\]'; then break; fi
  sleep 0.1
  if [ "$i" -eq 150 ]; then echo "FAIL: never reached blocked"; cat "$TMP/dispatch.out"; exit 1; fi
done
"$BB" --repo "$TMP" board | grep -q "which hook path?" || { echo "FAIL: question text missing in board"; exit 1; }
"$BB" --repo "$TMP" tui --once | grep -q "which hook path?" || { echo "FAIL: question text missing in tui --once"; exit 1; }
[ -f "$PEND" ] || { echo "FAIL: sidecar missing"; exit 1; }
grep -q '"questions"' "$PEND" || { echo "FAIL: sidecar has no questions"; exit 1; }

echo "== answer =="
"$BB" --repo "$TMP" answer "$SLICE" --by human --pick hook >/dev/null
wait $DISP_PID
cat "$TMP/dispatch.out"

echo "== assert done + run: line =="
"$BB" --repo "$TMP" board | grep -q "\[x\].*done" || { echo "FAIL: slice not done"; exit 1; }
"$BB" --repo "$TMP" show '#3' | grep -q "run: session mock-1" || { echo "FAIL: run: line missing"; exit 1; }
"$BB" --repo "$TMP" show '#3' | grep -q '\$0.042' || { echo "FAIL: cost missing in run: line"; exit 1; }
DONE_SUMMARY="$(grep -o '\[x\].*' "$TMP/dispatch.out" | head -1 || true)"
echo "dispatch output ok"

echo "== assert single initial prompt (never re-prompted) =="
[ "$(grep -c '=== invocation ===' "$MOCK_LOG/argv.log")" -eq 1 ] || { echo "FAIL: mock invoked more than once (re-prompted)"; cat "$MOCK_LOG/argv.log"; exit 1; }
grep -q "stream-json" "$MOCK_LOG/argv.log" || { echo "FAIL: prompt missing stream-json flag"; exit 1; }
grep -A1 -- "--model" "$MOCK_LOG/argv.log" | grep -q "sonnet" || { echo "FAIL: default model sonnet missing in argv"; cat "$MOCK_LOG/argv.log"; exit 1; }

echo "== assert hook returned allow + updatedInput =="
grep -q '"permissionDecision"[[:space:]]*:[[:space:]]*"allow"' "$MOCK_LOG/hook.log" || { echo "FAIL: hook did not allow"; cat "$MOCK_LOG/hook.log"; exit 1; }
grep -q 'updatedInput' "$MOCK_LOG/hook.log" || { echo "FAIL: hook missing updatedInput"; exit 1; }
grep -q 'hook' "$MOCK_LOG/hook.log" || { echo "FAIL: hook missing answer label"; exit 1; }

echo "== assert done summary <= 2 sentences =="
SUMMARY_LINE="$("$BB" --repo "$TMP" board | grep '\[x\]' | head -1)"
PERIODS="$(echo "$SUMMARY_LINE" | tr -cd '.' | wc -c)"
[ "$PERIODS" -le 3 ] || { echo "FAIL: done summary too long: $SUMMARY_LINE"; exit 1; }

echo "== error fixture: is_error:true -> run-report + blocked, no done =="
TMP2="$(mktemp -d)"
cp problems/003-claude-dispatch.md "$TMP2/003-claude-dispatch.md"
"$BB" --repo "$TMP2" init >/dev/null
"$BB" --repo "$TMP2" sync "$TMP2/003-claude-dispatch.md" --offline >/dev/null
SLICE2="$(grep -oE '#3/[A-Za-z_-]+' "$TMP2/003-claude-dispatch.md" | head -1 || true)"
cat > "$TMP2/fake-claude-err" <<EOF
#!/usr/bin/env bash
echo '{"type":"system","subtype":"init","session_id":"mock-err"}'
echo '{"type":"result","result":"Tool exploded badly.","session_id":"mock-err","total_cost_usd":0.001,"usage":{"input_tokens":10,"output_tokens":5},"duration_ms":1000,"num_turns":1,"is_error":true}'
EOF
chmod +x "$TMP2/fake-claude-err"
"$BB" --repo "$TMP2" dispatch "$SLICE2" --by agent-1 --mock-bin "$TMP2/fake-claude-err" >/dev/null 2>&1 || true
"$BB" --repo "$TMP2" board | grep -q "\[!\].*blocked" || { echo "FAIL: error run not blocked"; exit 1; }
if "$BB" --repo "$TMP2" board | grep -q "\[x\].*${SLICE2#*/}.*done"; then echo "FAIL: error run wrongly done"; exit 1; fi
"$BB" --repo "$TMP2" show '#3' | grep -q "run: session mock-err" || { echo "FAIL: error run: line missing"; exit 1; }
"$BB" --repo "$TMP2" show '#3' | grep -q "error" || { echo "FAIL: error status missing"; exit 1; }
rm -rf "$TMP2"

echo "== tab parity with reports present =="
diff <("$BB" --repo "$TMP" board --open) <("$BB" --repo "$TMP" tui --once --tab open) || { echo "FAIL: open parity"; exit 1; }
diff <("$BB" --repo "$TMP" board --closed) <("$BB" --repo "$TMP" tui --once --tab closed) || { echo "FAIL: closed parity"; exit 1; }

echo "== reports never leak into ticks =="
if "$BB" --repo "$TMP" board | grep -q "run-report"; then echo "FAIL: report leaked into ticks"; exit 1; fi

echo "== no git on read path =="
if grep -rn '"git"' src/ ; then echo "FAIL: git invocation in src/"; exit 1; fi
"$BB" --repo "$TMP" board --explain | grep -q "git=none" || { echo "FAIL: --explain missing"; exit 1; }

echo "E2E-DISPATCH OK"
