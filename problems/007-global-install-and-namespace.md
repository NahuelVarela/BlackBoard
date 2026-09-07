---
issue: 1
---

# #7 — Machine-wide install, git-repo namespaces, and a real `bb help`

`id: #7` | `status: open for planning` | `owner: human` | `refs: this repo, gh:TODO`

## Context / Why

`bb` today only works from inside this one checkout: you either
`cargo run --` from the `BlackBoard/` repo, or pass `--repo <path>` by hand.
`Store::new` roots `.blackboard/` at whatever the current directory happens
to be — not the repo you're standing in — so running `bb` from a
subdirectory silently creates a second, disconnected `.blackboard/`. There
is also no single place that explains how to use the tool: the workflow
lives in `README.md`, verb-level docs live in `--help`, and neither one
tells you *which* board you're currently talking to.

This breaks the moment blackboard is used for more than one repo. The
whole point of the tool (per #1) is "agents coordinate via a shared local
board" — that only scales across repos if (1) the `bb` binary is reachable
from anywhere on the machine, (2) each repo gets its own isolated board
automatically, keyed off the repo itself rather than the caller's cwd, and
(3) a human or agent landing on a machine for the first time can run one
command and understand both the tool and which board they're on.

This document is the single source for planning #7. A new agent should be
able to read this file alone and produce a plan.

## Goal

Three must-haves, all reinforcing the same idea: **one binary, many
repo-scoped boards, one discoverable entry point.**

1. **`bb help`** — a first-class command (also triggered by bare `bb`)
   that explains the tool end-to-end: human loop, agent loop, verb list,
   *and* the resolved namespace for the directory you ran it from
   (root path, how it was found, where state lives). `-h` / `--help` /
   `bb <verb> --help` keep clap's standard, script-safe output unchanged.
2. **Machine-wide install** — `bb` installable once (`cargo install --path .`,
   idiomatic for a Rust binary; `~/.cargo/bin` is already on `PATH` for any
   rustup user) so any shell or agent process anywhere on the machine can
   invoke `bb` by name, with no `--manifest-path`, no `cargo run`, no cwd
   requirement. Document a non-cargo fallback (`scripts/install.sh` copying
   the release binary to `~/.local/bin`).
3. **Git-repo namespace** — `bb`'s working directory (`.blackboard/`)
   resolves to the nearest ancestor directory containing `.git`, walked up
   from cwd — not raw cwd. Running `bb` from any subdirectory of a repo
   hits that repo's *one* board. `--repo <path>` remains an explicit,
   exact override (bypasses discovery entirely — required for tests/CI
   reproducibility, unchanged from #1-#3). Outside any git repo, `bb`
   falls back to a single machine-wide default namespace
   (`$XDG_DATA_HOME/blackboard/default`, or `~/.local/share/blackboard/default`)
   instead of erroring, but says so out loud.

Non-goals for #7:
- No daemon, no HTTP/MCP server, no cross-machine sync of namespaces
  (carried over from #1's non-goals — namespaces are still local-only).
- No Windows support (already out of scope per #3's deferred list; `.git`
  walk-up + `$HOME` lookup are POSIX-only in v1).
- No config file (`.blackboardrc` / `bb.toml`) for pinning a namespace —
  `--repo` is the only override in v1.
- No auto-modification of the user's shell `PATH` / rc files — installing
  onto `PATH` is the standard rustup/cargo contract, not `bb`'s job.
- No merging of git worktrees into one namespace — each worktree's own
  `.git` file (pointing at the main repo's git dir) is still a valid
  discovery marker, so each worktree gets its own board unless the human
  explicitly unifies them with `--repo`.
- No renaming of `--repo`'s existing exact-path semantics — every existing
  e2e script (`e2e_tabs.sh`, `e2e_dispatch.sh`) always passes `--repo`, so
  discovery must not change their behavior at all.

## Slices (work items — tick independently)

| Slice | ID | State | Actor | Summary |
|-------|----|-------|-------|---------|
| Namespace resolution (`.git` walk-up + default fallback) | #7/namespace | [x] done | claude-agent | done |
| Machine-wide install (`cargo install`, `scripts/install.sh`, docs) | #7/install | [x] done | tui-agent | done |
| Rich `bb help` (bare-`bb` + namespace/install self-report) | #7/help | [x] done | claude-agent | done |
| E2E: subdir discovery, no-git fallback, `--repo` override parity | #7/e2e | [x] done | claude-agent | done |

Example desired `bb help` output (from inside a repo subdirectory):

```
$ cd src/ && bb help
bb — token-efficient issue-driven coordination for humans + agents

Namespace: BlackBoard  (/home/bigwhite/repos/Blackboard/BlackBoard)
  found via: .git in that directory (you are in a subdirectory)
  state:     .blackboard/ (log.jsonl + index.db)

Human loop:
  bb init                          create/open this repo's board
  bb sync problems/<n>.md          .MD -> GitHub issue + board (never commits code)
  bb board / bb tui                watch progress (no git on the read path)

Agent loop:
  bb show '#N'                     slices + refs, ~200 tokens
  bb pick '#N/slice' --by <actor>  claim one slice
  bb done '#N/slice' --by <actor> --summary "<=2 sentences"

Install: /home/bigwhite/.cargo/bin/bb (on PATH: yes)
Run `bb <verb> --help` for flags on any command.
```

And outside any git repo:

```
$ cd /tmp && bb help
Namespace: (default)  (/home/bigwhite/.local/share/blackboard/default)
  found via: no .git in any ancestor of /tmp — using the machine-wide default
  state:     .blackboard/ (log.jsonl + index.db)
  tip: run bb from inside a git repo to get a repo-scoped board
...
```

## Namespace protocol (v1)

Resolution order, evaluated once per invocation:
1. `--repo <path>` given → use exactly that path, no `.git` check (today's
   behavior, unchanged; this is what every existing e2e script uses).
2. Else walk up from `cwd` (after canonicalizing) looking for a `.git`
   entry (directory *or* file — worktrees/submodules use a `gitdir:`
   pointer file) at each level. First hit wins; that directory is the
   namespace root.
3. Else (no `.git` found before the filesystem root) → the fixed default
   namespace root `$XDG_DATA_HOME/blackboard/default` (falling back to
   `$HOME/.local/share/blackboard/default` when `XDG_DATA_HOME` is unset).

`.blackboard/` is created under whichever root wins, exactly as it is
today under `--repo`. No tuple-schema change, no store-format change —
`Store` still just takes a root `Path`; only what computes that path
changes. `--explain` gains the resolved namespace on its output line
(root + how-found) alongside the existing `git=none` proof — this is a
namespace lookup via `.git`'s *presence*, not a `git` subprocess call, so
the "no git on the read/write path" invariant from #1 is unchanged.

## Agent workflow (unchanged)

`bb show` / `pick` / `state` / `done` / `dispatch` / `answer` work exactly
as in #1-#3. Namespace resolution only changes *where* `.blackboard/`
lives, never the tuple protocol or verb behavior.

## Tech decisions (decided)

- No new crates. `.git` walk-up and `$HOME`/`$XDG_DATA_HOME` lookup via
  `std::env` + `std::path` only — keeps the dependency footprint as
  minimal as #1-#3 intended (no `dirs`/`directories` crate for two env
  lookups).
- `bb help` is a real `Command::Help` variant (clap's *implicit* help
  subcommand is disabled via `disable_help_subcommand`) so it can print
  dynamic content (resolved namespace, install self-check) that
  `--help`/`-h` cannot (clap intercepts and exits before `main` logic
  runs for those). `cmd` becomes `Option<Command>`; bare `bb` (no
  subcommand) routes to the same handler as `bb help`.
- Distribution is `cargo install --path .` as the primary, documented
  path (idiomatic for a single Rust binary; installs to `~/.cargo/bin`,
  already on `PATH` for any rustup install). `scripts/install.sh` is a
  thin convenience wrapper (`cargo build --release` + copy to
  `~/.local/bin/bb`) for agents/environments without `cargo install`
  rights or that prefer XDG-style `~/.local/bin`. No installer script
  touches shell rc files.
- Install self-check in `bb help` compares `std::env::current_exe()`
  against a `PATH` scan for `bb` (first match) — read-only, no
  subprocess, same "no shelling out" discipline as the rest of the tool.

## Acceptance for #7

- [x] `bb help` (and bare `bb`) prints the workflow guide + resolved
  namespace (root, how found) + install self-check; `bb <verb> --help`
  and `bb --help`/`-h` remain clap-standard and unchanged in wording.
- [x] Running `bb init` (no `--repo`) from a subdirectory of this repo
  creates `.blackboard/` at the repo root, not the subdirectory; running
  it again from a different subdirectory hits the same board.
- [x] Running `bb` outside any git repo resolves to the fixed default
  namespace and says so; `--repo` still overrides both cases exactly as
  today (existing e2e scripts unaffected — they always pass `--repo`).
- [x] `bb board --explain` / `bb show --explain` include the resolved
  namespace line; `git=none` still holds (no `git` subprocess anywhere).
- [x] `cargo install --path .` produces a `bb` on `PATH` that works from
  any directory on the machine; README documents this + `scripts/install.sh`.
- [x] `cargo test` + `cargo build --release` green; new
  `tests/e2e_namespace.sh` covers subdir discovery, no-git fallback
  (with `XDG_DATA_HOME` pointed at a tmp dir so the test never touches
  the real machine-wide default), and `--repo` override parity.

## For the planner (next agent)

Input: this file only + repo listing (+ #1's plan for `Store`/`repo_root`
context, not as input). Output: `docs/plans/yyyy-mm-dd-007-plan.md` with
task breakdown per slice (namespace/install/help/e2e), file layout deltas
(`src/namespace.rs` new, `src/cli.rs` + `src/main.rs` deltas, `README.md` +
`scripts/install.sh` new), test plan (unit: walk-up from nested dirs,
worktree `.git`-file case, default-fallback path computation; e2e as
above), and what is deferred (config file, `PATH`-mutation, Windows,
worktree unification). Commit plan before implementing (per Talwrn
DevApproach).
