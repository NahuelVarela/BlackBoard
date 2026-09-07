---
issue: 1
---

# #6 — Sequential slice handoff (analytics -> backend -> frontend)

`id: #6` | `status: open for planning` | `owner: human` | `refs: this repo, gh:TODO`

## Context / Why

#1-#5 proved one dispatched agent can move one slice. Untested: whether
slice N+1 can pick up where slice N left off **with no human relaying
anything**. Today the only handoff channel is the `done` summary rendered
by `bb show` into the next agent's default prompt
(`default_prompt` -> `board::render_show`), capped at 280 chars by
`claude::first_two_sentences`.

This problem is the fire-test of exactly that channel. It is deliberately
built so the handoff CANNOT be guessed from this file: each slice chooses
an interface the next slice must consume, and this document does not name
that interface. If the handoff channel is too thin, backend and frontend
will produce broken output, and that is the result we want to observe.

## Goal

Three slices, dispatched strictly in order, each consuming the previous
slice's chosen interface. Everything lands under `demo/` so the Rust build
and `src/` are untouched.

Must-haves:
1. `#6/analytics` writes `demo/analytics/gen_events.sh` which generates
   `demo/data/events.jsonl` (>= 20 JSON-per-line records of website
   traffic events). **The analytics slice chooses the field names and the
   set of event types.** It documents them in `demo/analytics/SCHEMA.md`.
2. `#6/backend` writes `demo/backend/aggregate.sh` which reads
   `demo/data/events.jsonl` and writes `demo/data/summary.json`.
   It MUST use the field names analytics actually chose. **The backend
   slice chooses the shape of summary.json.**
3. `#6/frontend` writes `demo/frontend/render.sh` which reads
   `demo/data/summary.json` and prints an ASCII dashboard to stdout.
   It MUST use the keys backend actually chose.

Non-goals: no changes under `src/`, no new `bb` verbs, no Rust code, no
network. Bash + `jq` only (both present). Do not touch the working tree
outside `demo/`.

## Slices

- #6/analytics — event generator + schema it chooses
- #6/backend — aggregator over analytics' events, summary shape it chooses
- #6/frontend — ASCII dashboard over backend's summary

Dispatch order is strictly analytics -> backend -> frontend. A slice is
dispatched only after the previous one is `[x] done`.

## Agent workflow (dispatched slice)

1. You own EXACTLY ONE slice. It is the first `[ ] open` slice on the
   board render above your prompt; every `[x] done` slice is already
   finished by another agent — read its summary, do not redo its work.
2. Read `problems/006-sequential-handoff.md` for your slice's must-have.
3. If a previous slice chose an interface you must consume, discover it by
   reading the files that slice actually wrote under `demo/`.
4. Finish with a 2-sentence summary naming the interface YOU chose, so the
   next slice can consume it.

Do NOT use AskUserQuestion for this problem — every open decision is yours
to make. Decide, write it down, and finish.

## Acceptance for #6

- [ ] `bash demo/analytics/gen_events.sh && wc -l demo/data/events.jsonl` >= 20
- [ ] `bash demo/backend/aggregate.sh && jq . demo/data/summary.json` parses
- [ ] `bash demo/frontend/render.sh` prints a dashboard with real numbers
- [ ] End-to-end chain runs clean from empty `demo/data/`
- [ ] `git status --porcelain src/` is empty (no agent touched src/)
