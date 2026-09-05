---
issue: null # filled by `bb sync` — if null, sync creates a GitHub issue; if set, sync updates it
---

# #1 — Blackboard CLI MVP (token-efficient issue-driven coordination)

`id: #1` | `status: open for planning` | `owner: human` | `refs: this repo, gh:TODO`

## Context / Why

In a 10-engineer workshop using agents as fast as possible, the team
accidentally re-discovered the blackboard pattern (Hearsay-II / Linda
tuple-space): agents coordinated across users, sessions, laptops and
tasks by committing, rebasing and pushing very frequently, with plan
documents as first-class markdown, progress tables, stable work-item
ids, and commits stating who / what / goal.

On the way home from that workshop (Girona Airport), Talwrn was started
to make that accidental blackboard an actual blackboard.

This repo (`Blackboard`) is a test of that idea, focused on one loop:

> human writes problem `.MD` -> deterministic sync to GitHub + blackboard
> -> agents pick slices, plan, execute -> publish `done` once + 2-sentence summary.

Current pain: calling `git` for everything is token-expensive. `push/pull`
is fine. Bashing `git log/show` for local development / status reads is not.
The board must be readable without git on the hot path.

This document is the single source for planning #1. A new agent should be
able to read this file alone and produce a plan. Humans edit this file
directly with no agent (skill-assisted drafting is OK, deterministic sync is required).

## Goal

A local-first CLI `bb` that lets a human visualize progress and lets agents
coordinate on slices of a problem with minimal tokens.

Must-haves for MVP:
1. Append-only tuple log, local-first, no git on read path. Everything stays local except GitHub Issues.
2. Slice-level ticks: `[x] dbt [x] backend [ ] frontend` per problem.
3. Slice state machine visible in CLI + TUI: `open -> planning -> executing -> done`, plus `blocked`.
4. Deterministic `sync`: `.MD` -> GitHub Issue create-or-update + blackboard `issue-opened`, zero LLM tokens. Sync NEVER commits code. Code/commits/PRs happen after work is done and link to the issue via commit message (`Refs #<n>`).
5. Agent protocol: pick 1 slice, publish once at done with <=2-sentence summary. Commit message includes issue number from blackboard.

Non-goals for #1:
- No daemon, no HTTP API, no web UI, no MCP server, no auth.
- No destructive `take`. No atomic distributed lock.
- No transport of board data itself outside GitHub Issues. No orphan branch, no background pusher in #1. Board is local-only.
- No formal models (Alloy/TLA+) yet. Note as future.

## Slices (work items — tick independently)

| Slice | ID | State | Actor | Summary |
|-------|----|-------|-------|---------|
| Core tuple log (JSONL append + schema) | #1/core | [ ] open | — | — |
| CLI verbs with `clap` (init/assert/query/state/done/board/show) | #1/cli | [ ] open | — | — |
| Local indexer + board views (no git reads) | #1/board | [ ] open | — | — |
| TUI board (`ratatui`, same projection, watch mode) | #1/tui | [ ] open | — | — |
| Deterministic issue sync (gh create-or-update only, no code commits) | #1/sync | [ ] open | — | — |

Example desired view for this issue itself once worked (CLI and TUI show same):

```
#1 blackboard-cli
  [ ] core   open       —  waiting pick
  [ ] cli    open       —  waiting pick
  [ ] board  open       —  waiting pick
  [ ] tui    open       —  waiting pick
  [ ] sync   open       —  waiting pick
  refs: problems/001-blackboard-cli.md | gh#1
```

Later, with agents working:

```
#1 blackboard-cli
  [~] core   executing  agent-1  "JSONL segments per writer, schema validated"
  [.] cli    planning   agent-2  "clap derive layout, verbs sketched"
  [ ] board  open       —        waiting pick
  [ ] tui    open       —        waiting pick
  [ ] sync   open       —        waiting pick
```

Legend: `[ ]` open, `[.]` planning, `[~]` executing, `[x]` done, `[!]` blocked.

## Tuple protocol (v1)

Tuple-space blackboard, no pre-defined domain schema, but fixed envelope:

```json
{"id":"#1/core","type":"slice-state","state":"open|planning|executing|done|blocked","actor":"human|agent-1","summary":"<=2 sentences","ts":"RFC3339","refs":["problems/001-blackboard-cli.md","gh://owner/repo/issues/1"]}
```

Rules:
- Append-only. Never edit history. New state = new tuple.
- Current state = tuple with max `ts`, tiebreak by `actor` lexicographically. Every replica computes identically.
- `open` comes only from deterministic `bb sync`. Agents may assert `planning|executing|blocked|done` only.
- `done` MUST include `summary` (1-2 sentences, what changed + how to verify). No diff dump.
- `blocked` MUST include reason + what unblocks in `summary`.
- `refs` MUST link back to this file and (once created) the GitHub issue/comment. Hypertext, not duplication. Code stays in repo, coordination stays in board.

## Agent workflow (skill: `Pick #N slice, plan and execute`)

1. `bb show #1` — read slices + refs (local indexer, ~200 tokens, never `git log`).
2. `bb pick #1/<slice> --by <actor>` — asserts `planning`. One slice per agent.
3. Work locally. `cargo test`, `dbt build`, etc. No pushes, no board spam.
4. Optional single heartbeat: `bb state #1/<slice> executing --summary "..."`.
5. At done: `bb done #1/<slice> --summary "<=2 sentences>"` — asserts `done` = positive tick. Stop.
6. When committing code after done, include issue number from blackboard in commit message, e.g. `feat(board): ticks view Refs #1`. Board never commits code itself.

Skills MAY use agents to help draft `problems/*.md`. Sync MUST be deterministic (script, not LLM).

## Deterministic sync (human half, 0 agent tokens)

`bb sync` syncs ONLY issues to GitHub. It NEVER adds/commits/pushes code.
Code/commits/PRs are handled separately after work is done and MUST link
to the issue (commit message `Refs #<n>`).

```
edit problems/001-blackboard-cli.md (human + skill, free)
  -> bb sync problems/001-blackboard-cli.md   # deterministic, no LLM
       ├─ parse frontmatter: `issue:` field (empty = no issue yet)
       ├─ if no issue: `gh issue create --title ... --body-file ...` -> save returned #n into frontmatter + blackboard
       ├─ if issue exists: `gh issue edit <n>` / comment plan mirror update
       └─ bb assert issue-opened for #1 (stores gh issue #n as THE issue for this plan) + slice open for #1/core,#1/cli,#1/board,#1/tui,#1/sync
```

The GitHub issue number is then info in the blackboard. Agents read it via
`bb show` and use it in commit messages. If a plan has NO issue, sync creates
one. If it HAS one, sync updates it.

`bb sync` does not shell to `git commit/push` for code. Agents never run it in the loop.

## Tech decisions (decided)

- Language: Rust — chosen. Rationale: 5ms startup on hot path (agents shell out per op), single static binary, strict types prevent tuple corruption.
- CLI: `clap v4` derive. Commands: `init, sync, pick, state, done, show, board`.
- Store: local-only `.blackboard/log.jsonl` + `redb` or `rusqlite` indexer. Reads hit indexer only. No transport of board data outside GitHub Issues in #1.
- TUI: in scope for #1. `ratatui + crossterm` over same projection as `bb board`, with watch mode. This is what the human watches. Web/MCP/HTTP explicitly deferred.
- Transport: local-only for #1. No orphan branch, no background pusher. GitHub Issues are the only external transport.

Rust is NOT installed in this env yet (`cargo` missing). Plan must include `rustup` bootstrap step.

## Acceptance for #1

- [ ] `bb board` shows ticks above without invoking `git` (prove with `--explain` or log).
- [ ] `bb tui` shows the same ticks in a watchable TUI (ratatui) — human-visible proof.
- [ ] Human loop: edit MD -> `bb sync` -> GitHub issue created (null -> #n saved to frontmatter + blackboard) or updated (issue set -> `gh issue edit`), `bb show #1` shows 5 slices open + stored issue #. Sync touches NO code files.
- [ ] Agent loop works with 2 agents on 2 slices concurrently, each publishing once, board+TUI show `[~]/[x]` correctly. Commits after done include `Refs #<n>` from blackboard.
- [ ] Token note in README/docs: measured `bb show` output <= ~40 lines for #1.
- [ ] `cargo build --release` produces single `bb` binary. Local-only, no board transport.

## For the planner (next agent)

Input: this file only + repo listing. Output: `docs/plans/yyyy-mm-dd-001-plan.md` with task breakdown per slice (core/cli/board/tui/sync), file layout (`src/main.rs`, `src/tuple.rs`, `src/store.rs`, `src/cli.rs`, `src/tui.rs`), test plan (unit + genuine e2e: 2 agents simulated via shell, TUI snapshot/manual check), and what is deferred (daemon/HTTP/MCP/orphan-branch transport). Commit plan before implementing (per Talwrn DevApproach). Ask 2-4 questions max if blocked (store choice redb vs sqlite only — Rust, TUI-in-scope, and local-only are decided).
