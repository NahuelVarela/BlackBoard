#!/usr/bin/env bash
# E2E for plan #7: git-root discovery, no-git default fallback, --repo
# override parity, install-script smoke test. Genuine e2e — real process,
# real filesystem, no `git` subprocess anywhere (bb only stats `.git`).
set -euo pipefail
BB="$(realpath "${BB:-./target/debug/bb}")"
ROOT_DIR="$(pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "== 1. subdir discovery: .blackboard/ lands at the git root, not cwd =="
REPO="$TMP/repo"
mkdir -p "$REPO/a/b/c"
(cd "$REPO" && git init -q)
(cd "$REPO/a/b/c" && "$BB" init >/dev/null)
[ -d "$REPO/.blackboard" ] || { echo "FAIL: .blackboard/ not created at git root"; exit 1; }
[ -d "$REPO/a/b/c/.blackboard" ] && { echo "FAIL: .blackboard/ leaked into subdirectory"; exit 1; }
(cd "$REPO/a/b" && "$BB" init >/dev/null) # second run, different subdir, same repo
NS_LINE="$(cd "$REPO/a/b/c" && "$BB" help | grep '^Namespace:')"
echo "$NS_LINE"
echo "$NS_LINE" | grep -q "$REPO" || { echo "FAIL: help namespace line doesn't name repo root"; exit 1; }
echo "$NS_LINE" | grep -qv "a/b/c" || { echo "FAIL: help namespace line leaked the subdirectory"; exit 1; }

echo "== 2. no-git fallback: isolated XDG_DATA_HOME, never touches the real machine default =="
NOGIT="$TMP/nogit"
XDG="$TMP/xdg"
mkdir -p "$NOGIT" "$XDG"
HELP_OUT="$(cd "$NOGIT" && XDG_DATA_HOME="$XDG" "$BB" help)"
echo "$HELP_OUT" | grep -q "^Namespace: (default)" || { echo "FAIL: no-git case didn't report default namespace"; exit 1; }
echo "$HELP_OUT" | grep -q "$XDG/blackboard/default" || { echo "FAIL: default namespace not rooted under isolated XDG_DATA_HOME"; exit 1; }
(cd "$NOGIT" && XDG_DATA_HOME="$XDG" "$BB" init >/dev/null)
[ -d "$XDG/blackboard/default/.blackboard" ] || { echo "FAIL: init under no-git fallback didn't land in XDG default"; exit 1; }

echo "== 3. --repo still overrides discovery exactly (existing e2e scripts rely on this) =="
OTHER="$TMP/other-explicit"
mkdir -p "$OTHER"
EXPLAIN="$(cd "$REPO/a/b/c" && "$BB" --repo "$OTHER" board --explain 2>&1 || true)"
echo "$EXPLAIN" | grep -q "namespace: $OTHER (explicit --repo)" || { echo "FAIL: --repo did not short-circuit discovery"; exit 1; }
echo "$EXPLAIN" | grep -q "git=none" || { echo "FAIL: git=none proof missing from --explain"; exit 1; }

echo "== 4. no git subprocess anywhere on the namespace path =="
if grep -rn '"git"' src/ | grep -v '\.git' >/dev/null; then
  echo "FAIL: found a literal \"git\" subprocess invocation in src/"; exit 1
fi

echo "== 5. install.sh smoke test: builds + installs to a redirected dir =="
INSTALL_DIR="$TMP/install-bin"
BB_INSTALL_DIR="$INSTALL_DIR" "$ROOT_DIR/scripts/install.sh" >/dev/null
[ -x "$INSTALL_DIR/bb" ] || { echo "FAIL: scripts/install.sh did not produce an executable bb"; exit 1; }
"$INSTALL_DIR/bb" --repo "$OTHER" board >/dev/null || { echo "FAIL: installed binary did not run"; exit 1; }

echo "OK: e2e_namespace.sh"
