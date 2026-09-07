# BlackBoard

Blackboard CLI (`bb`) MVP — plan [#1](problems/001-blackboard-cli.md):
token-efficient issue-driven coordination for humans + agents.

## Install

```sh
cargo install --path .   # installs `bb` to ~/.cargo/bin, already on PATH for any rustup user
```

This is the primary, idiomatic path for a single Rust binary — no
`--manifest-path`, no `cargo run`, no cwd requirement afterward; `bb` is
then reachable by name from any shell or agent process on the machine.

If `cargo install` isn't available (e.g. an agent sandbox without cargo's
install directory writable), use the fallback script instead:

```sh
scripts/install.sh                        # builds + copies to ~/.local/bin/bb
BB_INSTALL_DIR=/some/other/dir scripts/install.sh   # override install dir
```

Requires Rust stable (`rustup`). `rusqlite` uses the `bundled` feature —
no system SQLite needed. Neither install path touches your shell's
`PATH`/rc files — that's `rustup`'s job, not `bb`'s.

`.blackboard/` lives at your nearest git repo root (walked up from cwd),
so one `bb` binary serves many repos, each with its own board — run `bb
help` to see which board you're currently on.

## Human loop

```sh
bb init
# edit problems/001-blackboard-cli.md (human + skill, free)
bb sync problems/001-blackboard-cli.md   # deterministic: gh issue create-or-update + board asserts, NEVER commits code
bb board --explain                        # ticks; proves indexer-only reads (git=none)
bb tui                                    # watchable board (same projection), q to quit
```

## Agent loop

```sh
bb show '#1'                              # slices + refs (~200 tokens, never git log)
bb pick '#1/core' --by agent-1            # asserts planning (one slice per agent)
# ... work locally: cargo test, etc. No pushes, no board spam.
bb state '#1/core' executing --by agent-1 --summary "..."   # optional heartbeat
bb done '#1/core' --by agent-1 --summary "<=2 sentences>"   # positive tick, then stop
# commit code after done with: Refs #<n> (issue number from `bb show`)
```

Legend: `[ ]` open, `[.]` planning, `[~]` executing, `[x]` done, `[!]` blocked.

## Token note (measured, #1)

`bb show '#1'` output is **7 lines** (5 slices + header + refs) — well under
the ~40-line budget. `bb board` adds one legend line per repo. All reads hit
`.blackboard/index.db` only; `--explain` prints `git=none` as proof.

## Layout

- `src/tuple.rs` — envelope + validation + current-state reduction
- `src/store.rs` — `.blackboard/log.jsonl` append + `rusqlite` indexer
- `src/cli.rs` — `clap` verbs (`init/pick/state/done/show/board/sync/tui`)
- `src/board.rs` — projection shared by CLI and TUI
- `src/tui.rs` — `ratatui` watch mode (`--once` for CI snapshots)
- `src/sync.rs` — deterministic `.MD` -> `gh issue` sync (no code commits)
- `src/namespace.rs` — resolves `.blackboard/`'s root (`--repo` > git walk-up > XDG default)
- `src/help.rs` — `bb help` / bare `bb`: workflow guide + resolved namespace + install self-check
- `tests/e2e_two_agents.sh` — genuine e2e: 2 concurrent agents, TUI parity, no-git check
- `tests/e2e_namespace.sh` — genuine e2e: subdir discovery, no-git default fallback, `--repo` override parity, install script smoke test
