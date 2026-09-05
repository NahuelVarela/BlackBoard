---
issue: null # filled by `bb sync` — if null, sync creates a GitHub issue; if set, sync updates it
---

# #2 — TUI open/closed tabs (finish the done-loop)

`id: #2` | `status: open for planning` | `owner: human` | `refs: this repo, gh:TODO`

## Context / Why

Plan #1 shipped the `bb` MVP: local-first tuple log, CLI verbs, board views,
ratatui TUI, deterministic sync. It works — #1's own board proves it: all 5
slices are `[x] done`.

But the TUI has no notion of finished work. Open and closed slices render in
one flat list, so a fully-done problem (#1 right now) looks the same as a
fully-open one at a glance. The human watching the board wants two answers
fast: "what still needs attention?" and "what's finished?" — without parsing
glyphs line by line.

This document is the single source for planning #2. A new agent should be
able to read this file alone and produce a plan. Humans edit this file
directly with no agent (skill-assisted drafting is OK, deterministic sync is required).

## Goal

Tabs in `bb tui`: **Open** and **Closed**, over the same projection the CLI uses.

Must-haves for #2:
1. Tab bar: `Open (n) | Closed (m)` with live counts, same tick glyphs as #1.
2. Partition: Open = `open|planning|executing|blocked`; Closed = `done`. No tuple-schema change — states already exist.
3. Keybindings: `Tab` (or `1`/`2`, `←`/`→`) switches tabs, `q`/`Esc` still quits. Watch mode keeps polling per tab.
4. Empty states: Open-empty shows `(empty — everything is done)`; Closed-empty shows `(empty — nothing closed yet)`.
5. CLI parity: `bb board [--open|--closed]` prints the matching tab's content; `bb tui --once [--tab open|closed]` snapshots it for CI.
6. No git on the read path (still). `--explain` keeps proving `git=none`.

Non-goals for #2:
- No third tab (e.g. separate `Blocked`), no search/filter box, no sorting options, no mouse support.
- No TUI editing (no state transitions from inside the TUI — agents keep using verbs).
- No change to the tuple envelope or sync behavior.

## Slices (work items — tick independently)

| Slice | ID | State | Actor | Summary |
|-------|----|-------|-------|---------|
| Open/Closed partition in shared projection | #2/filter | [ ] open | — | — |
| Tab bar UI + keybindings + counts (ratatui) | #2/tabs | [ ] open | — | — |
| CLI parity (`board --open/--closed`, `tui --once --tab`) | #2/cli | [ ] open | — | — |
| E2E: mixed board + tab snapshots + empty states | #2/e2e | [ ] open | — | — |

Example desired TUI for #1's board (all done) once worked:

```
┌ Blackboard ──────────────────────────┐
│ [Open (0)]  Closed (5)                │
│                                       │
│  (empty — everything is done)         │
│                                       │
│ q / Esc to quit — watch 1000ms        │
└───────────────────────────────────────┘
```

And with agents mid-flight on a fresh problem:

```
┌ Blackboard ──────────────────────────┐
│ [Open (3)]  Closed (2)                │
│                                       │
│  [~] core   executing  agent-1  "..." │
│  [.] cli    planning   agent-2  "..." │
│  [ ] board  open       —  waiting pick│
│                                       │
│ q / Esc to quit — watch 1000ms        │
└───────────────────────────────────────┘
```

CLI parity for the same:

```
$ bb board --open
#3 something
  [~] core   executing  agent-1  "..."
  ...
```

Legend (unchanged): `[ ]` open, `[.]` planning, `[~]` executing, `[x]` done, `[!]` blocked.

## Tuple protocol (v1 — unchanged)

No envelope change. Tabs are a pure read-side partition over existing
`slice-state` tuples; current-state reduction (max `ts`, tiebreak `actor`)
is untouched. #2 adds zero tuple types and zero writes.

## Agent workflow (unchanged)

`bb show` / `pick` / `state` / `done` work exactly as in #1. Tab selection
never affects writes.

## Deterministic sync (unchanged)

`bb sync` behavior is untouched. It still NEVER commits code.

## Tech decisions (decided)

- Tabs via `ratatui::widgets::Tabs`, same `board::projection()` as #1 plus a
  `partition_open_closed()` helper in `src/board.rs` (shared by TUI + CLI).
- Selected-tab state lives in the TUI event loop only (not in the store).
- `bb board --open|--closed` are mutually exclusive flags; default (neither)
  keeps the #1 full-board output byte-identical.
- `bb tui --once --tab <open|closed>` defaults to `open`? No — defaults to
  full board (current `--once` behavior), `--tab` narrows it.

Rust, `clap v4`, `rusqlite{bundled}`, local-only: all carried over from #1.

## Acceptance for #2

- [ ] `bb tui` on #1's board (all done) shows `Open (0)` empty-state and `Closed (5)` with 5× `[x]`; `Tab`/`1`/`2` switch, counts update live.
- [ ] Mixed-state fixture (2 done + 3 open) renders the partitioned lists correctly in both tabs.
- [ ] `bb board --open` / `--closed` output equals the corresponding `bb tui --once --tab ...` content.
- [ ] `bb board` (no flags) output unchanged from #1.
- [ ] `--explain` still proves indexer-only reads; e2e extended in `tests/e2e_two_agents.sh` (or `e2e_tabs.sh`).
- [ ] `cargo test` + `cargo build --release` green.

## For the planner (next agent)

Input: this file only + repo listing (+ #1's plan for context, not as input).
Output: `docs/plans/yyyy-mm-dd-002-plan.md` with task breakdown per slice
(filter/tabs/cli/e2e), file layout deltas (`src/board.rs` partition helper,
`src/tui.rs` tab state + keybindings, `src/cli.rs` flags), test plan (unit:
partition correctness incl. blocked∈open; genuine e2e: mixed board fixture,
`--once --tab` snapshots, empty-state strings), and what is deferred
(third tab, search/sort, mouse, in-TUI transitions). Commit plan before
implementing (per Talwrn DevApproach). Dogfood fixture: #1's real board,
currently all `[x] done`.
