# BlackBoard

Blackboard CLI (`bb`) MVP — plan [#1](problems/001-blackboard-cli.md):
token-efficient issue-driven coordination for humans + agents.

## Build

```sh
cargo build --release   # single `bb` binary at target/release/bb (~4MB, 5ms-class startup)
```

Requires Rust stable (`rustup`). `rusqlite` uses the `bundled` feature —
no system SQLite needed.

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
- `tests/e2e_two_agents.sh` — genuine e2e: 2 concurrent agents, TUI parity, no-git check
