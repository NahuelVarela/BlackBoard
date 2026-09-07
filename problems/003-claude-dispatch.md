---
issue: null # filled by `bb sync` — if null, sync creates a GitHub issue; if set, sync updates it
---

# #3 — Claude Code dispatch from the blackboard (Claude-only)

`id: #3` | `status: open for planning` | `owner: human` | `refs: this repo, gh:TODO`

## Context / Why

Plans #1 (CLI + board + TUI) and #2 (Open/Closed tabs) shipped a
human-watchable board with slice states
`open -> planning -> executing -> done`, plus `blocked`:

Legend: `[ ]` open, `[.]` planning, `[~]` executing, `[x]` done, `[!]` blocked.

What is missing: the human cannot yet **launch a Claude Code agent on a
slice from the board** and watch it tick. Today dispatch is manual shell
(`claude -p "..."` in another terminal) and the board only moves when the
agent (or human) remembers to run `bb state/done` by hand. Questions from
the agent never reach the board.

Headless-only dispatch is a trap for both harnesses — plain
`claude -p` / `opencode run` cannot dispatch+surface questions without an
event loop. This plan covers **Claude Code only** (opencode deferred).
Known Claude facts driving the design:

- Plain `claude -p` does not surface `AskUserQuestion`: it is denied in
  `dontAsk` mode, unavailable to subagents via the Agent tool, and in
  headless/no-TTY it currently auto-resolves with empty answers and
  continues on assumptions (open bugs #64265/#50728).
- Supported surfacing paths are (a) Agent SDK `query({canUseTool})`:
  when `toolName == AskUserQuestion`, render `input.questions[]` in your
  own UI and return
  `{behavior:"allow", updatedInput:{questions, answers:{questionText:label}}}`;
  or (b) CLI-only: `PreToolUse` hook returning
  `permissionDecision:"allow" + updatedInput`, or
  `--permission-prompt-tool` + `--resume` loop.
- So the pattern for #3 is the CLI-only (b) path: **dispatch, keep the
  event loop running, on question event flip the cell to `[!] blocked` +
  show the prompt, feed the answer back as a tool-result / permission
  response — never as a new prompt.**
- Tokens at finish are first-class: `claude -p --output-format json`
  (final line) or `--output-format stream-json` (final message) emits
  `type:result {result, session_id, total_cost_usd/cost_usd,
  usage:{input_tokens,output_tokens,cache_*}, modelUsage, duration_ms,
  num_turns, is_error}`. #3 accumulates nothing — it parses that one
  object.

This document is the single source for planning #3. A new agent should be
able to read this file alone and produce a plan. Humans edit this file
directly with no agent (skill-assisted drafting is OK, deterministic sync is required).

## Goal

`bb dispatch` launches a Claude Code agent on one slice; the board ticks
itself (`planning -> executing -> blocked? -> done`) and records cost/tokens.

Must-haves for #3:
1. One verb: `bb dispatch '#3/<slice>' --by <actor> --prompt "..."`
   (prompt defaults to `bb show` slice context). Asserts `planning`, then
   `executing`, spawns
   `claude -p --output-format stream-json` as a child, records `session_id`.
   One slice per agent (same rule as `bb pick`).
2. Event loop, not fire-and-forget: `bb dispatch` (or a `bb dispatch --watch`
   sibling) owns the child stdio, parses stream-json events until the final
   `type:result`. Killing the loop kills/pauses the child; `--resume
   <session_id>` continues it.
3. Question surfacing (CLI-only, no SDK in #3): on `AskUserQuestion`
   tool-use event, the loop appends
   `bb state '#3/<slice>' blocked --summary "<question + what unblocks>"`
   so the cell flips to `[!]`, persists the full `questions[]` JSON to a
   sidecar (`.blackboard/pending/<slice>.json`), and feeds the human's
   answer back as a tool-result / permission response via the
   `PreToolUse` hook or `--permission-prompt-tool` mechanism — not as a
   new prompt. `bb answer '#3/<slice>' --pick "<label>"` (or `--text`)
   supplies the answer; the loop resumes the session.
4. Finish accounting: on final `type:result`, the loop appends a `done`
   tuple (existing `bb done` rule: <=2-sentence summary, what changed + how
   to verify) **plus** a machine-readable cost record (new tuple type,
   see below) with `{session_id, cost_usd, input/output/cache tokens,
   duration_ms, num_turns, is_error}`. `bb show` displays it; board glyphs
   are untouched.
5. TUI (read-side): `[!]` cell shows the pending question inline (first
   question text, truncated); full text available via `bb show` / sidecar.
   Answering happens via `bb answer` CLI — no in-TUI editing in #3.
6. No git on the read/write path (still). `--explain` keeps proving
   `git=none`. No new daemon, no HTTP, no MCP, no auth changes. Mockable:
   e2e uses a fake `claude` shim emitting canned stream-json, zero API spend.

Non-goals for #3:
- No opencode support (separate future plan; the `opencode serve` + SDK
  event path is documented in the issue thread, not built here).
- No Agent SDK (TypeScript `query({canUseTool})`) dependency — CLI-only
  hook path first. SDK loop is the explicit #4 candidate.
- No in-TUI answering/editing, no multi-slice fan-out in one command, no
  background daemon/scheduler, no prompt-injection hardening beyond
  passing through a fixed allowlist of tools.
- No change to the `slice-state` envelope or current-state reduction.

## Slices (work items — tick independently)

| Slice | ID | State | Actor | Summary |
|-------|----|-------|-------|---------|
| Dispatch verb + stream-json event loop (spawn, session_id, result) | #3/dispatch | [ ] open | — | — |
| Blocked surfacing (AskUserQuestion -> [!] + hook/permission-tool answer-back) | #3/blocked | [ ] open | — | — |
| Finish tokens (type:result -> run-report tuple + done) | #3/tokens | [ ] open | — | — |
| TUI + show read-side (pending question inline, cost line) | #3/tui | [ ] open | — | — |
| E2E with mocked claude shim (question + result fixtures, zero spend) | #3/e2e | [ ] open | — | — |

Example desired board while a dispatched agent is asking:

```
#3 claude-dispatch
  [~] dispatch executing  agent-1  "claude session abc123 streaming stream-json"
  [!] blocked  blocked    agent-1  "Q: which hook path? unblocks on bb answer (hook|prompt-tool)"
  [ ] tokens   open       —        waiting pick
  [ ] tui      open       —        waiting pick
  [ ] e2e      open       —        waiting pick
  refs: problems/003-claude-dispatch.md | gh#3
```

And at finish (`bb show '#3'` gains a cost line from the run-report tuple):

```
#3 claude-dispatch
  [x] dispatch done  agent-1  "stream-json loop landed, verify with mocked e2e."
  ...
  run: session abc123 | $0.042 | 12k in / 3k out (1k cache-read) | 14 turns | 92s | ok
  refs: problems/003-claude-dispatch.md | gh#3
```

Legend (unchanged): `[ ]` open, `[.]` planning, `[~]` executing, `[x]` done, `[!]` blocked.

## Tuple protocol (v1 + one additive type)

`slice-state` envelope and reduction (max `ts`, tiebreak `actor`) are
untouched. #3 adds one **read-ignored** tuple type for machine-readable
finish records (board projection skips it, so ticks never break):

```json
{"id":"#3/dispatch","type":"run-report","state":"done","actor":"agent-1","summary":"session abc123 ok, 14 turns, $0.042","ts":"RFC3339","refs":["problems/003-claude-dispatch.md","gh#3"],"extra":{"session_id":"abc123","cost_usd":0.042,"input_tokens":12000,"output_tokens":3000,"cache_read":1000,"cache_write":0,"duration_ms":92000,"num_turns":14,"is_error":false,"model":"sonnet"}}
```

Rules:
- `run-report` is written once per dispatch finish by the loop itself
  (parsed from the final `type:result`, never hand-typed). `id` matches the
  slice; `extra` carries the numbers; `summary` stays human-readable (<=2
  sentences not required — it is generated, and `bb show` renders it as the
  `run:` line).
- `blocked` for questions keeps the v1 rule: `summary` MUST contain the
  question (truncated to ~280 chars) + what unblocks
  (`unblocks on bb answer ...`). Full `questions[]` JSON lives in
  `.blackboard/pending/<problem>-<slice>.json` (sidecar, gitignored,
  never in the tuple).
- Store validation must accept the new type (today only
  `slice-state|issue-opened` pass). Indexer stores it; `projection()`
  ignores unknown types (already does — only `slice-state` builds ticks).
- `done` still comes from the loop via the existing `bb done` path with a
  human-grade summary derived from `result` (first ~2 sentences).

## Agent workflow (dispatched slice)

1. Human (or orchestrator): `bb dispatch '#3/<slice>' --by agent-1 --prompt "..."`
   — loop asserts `planning` then `executing`, spawns claude, streams events.
2. Agent works inside `claude -p` with the slice prompt (which includes
   `bb show` context + working agreement: use `AskUserQuestion` when blocked,
   never assume).
3. On question: loop asserts `blocked` + writes sidecar; human watches TUI
   (`[!]` + question) and runs `bb answer '#3/<slice>' --pick "<label>"`;
   loop feeds it back as tool-result/permission response and resumes
   (`--resume <session_id>`), flipping back to `executing`.
4. At finish: loop writes `run-report` + `done`, prints the `run:` line, exits.
   Code commits after done link `Refs #3` (unchanged rule).

Skills MAY draft `problems/*.md`. Sync MUST stay deterministic (script, not LLM).

## Deterministic sync (unchanged, table-parsed slices)

`bb sync` still NEVER commits code. Slice-open asserts come from this
file's Slices table (`#3/...` IDs under `## Slices`); the hardcoded set is
fallback only (per #2). Syncing this file asserts exactly
dispatch/blocked/tokens/tui/e2e open.

## Tech decisions (decided)

- Claude-only. CLI-only surfacing first: `claude -p --output-format
  stream-json` child + `PreToolUse` hook and/or `--permission-prompt-tool
  <bb-hook>` + `--resume` loop. No Node/TS SDK dependency in #3 (deferred to #4).
- Rust in `bb` (same binary, `std::process::Command` + line-delimited JSON
  parsing with existing `serde_json`; no new crates except possibly a tiny
  `--mock` test hook). Hook helper is a `bb` subcommand (`bb claude-hook`)
  so claude invokes the same binary — no second artifact.
- Pending-question sidecar: `.blackboard/pending/<N>-<slice>.json`
  (full `questions[]` + `session_id` + tool-use id). Gitignored. Tuples
  never carry full JSON.
- Answer verb: `bb answer '#3/<slice>' --pick "<option label>"` (single-
  question v1; multi-question = repeat or `--all-json`). Writes sidecar
  answer; the running loop picks it up and resumes. Standalone `bb answer`
  with no running loop errors clearly.
- Finish parse: final stream-json line with `type:"result"` is authoritative.
  Fields mapped 1:1 into `run-report.extra`. `is_error:true` still writes
  `run-report` but asserts `blocked` (not `done`) with the error in summary.
- `bb show`/`board`/`tui --once` stay indexer-only; `--explain` proves
  `git=none`. `rg '"git"' src/` stays empty (the child is `claude`, not `git`).

Rust, `clap v4`, `rusqlite{bundled}`, local-only: all carried over from #1/#2.

## Acceptance for #3

- [ ] `bb dispatch` on a slice with a mocked `claude` (canned stream-json:
  assistant -> AskUserQuestion -> answer -> `type:result`) drives
  `planning -> executing -> blocked -> executing -> done` with zero API spend.
- [ ] Blocked cell shows `[!]` + question text in `bb board` / `bb tui --once`;
  full `questions[]` round-trips through the sidecar; the answer is fed back
  as tool-result/permission response (assert via mock hook log), never as a
  new prompt (assert mock received exactly one initial prompt).
- [ ] Finish writes `run-report` with `session_id/cost/usage/turns/is_error`
  parsed from `type:result`; `bb show` renders the `run:` line; `done`
  summary is <=2 sentences.
- [ ] `is_error:true` fixture writes `run-report` + `blocked` (not `done`).
- [ ] `bb board` (no flags) shape unchanged; Open/Closed tabs partition
  unaffected (`blocked` in Open, `done` in Closed); `run-report` never leaks
  into ticks.
- [ ] `--explain` still proves indexer-only reads; `cargo test` +
  `cargo build --release` green; new e2e (`tests/e2e_dispatch.sh`) green offline.

## For the planner (next agent)

Input: this file only + repo listing (+ #1/#2 plans for context, not as input).
Output: `docs/plans/yyyy-mm-dd-003-plan.md` with task breakdown per slice
(dispatch/blocked/tokens/tui/e2e), file layout deltas (`src/dispatch.rs` or
`src/claude.rs` event loop, `src/cli.rs` verbs, `src/store.rs` new-type
acceptance, `src/board.rs` `run:` line, `src/tui.rs` pending inline,
`tests/e2e_dispatch.sh` mock shim), test plan (unit: result parsing incl.
`is_error`, sidecar round-trip, validation of `run-report`; genuine e2e:
mocked question->answer->result flow, single-prompt assertion, tab parity),
and what is deferred (opencode path, Agent SDK loop, in-TUI answering,
fan-out, daemon). Commit plan before implementing (per Talwrn DevApproach).
Dogfood: dispatch a real `claude -p` on a docs-only slice after mocks pass.
