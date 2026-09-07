---
issue: 1
---

# #4 — Hello world that asks 3 questions

`id: #4` | `status: open for planning` | `owner: human` | `refs: this repo, gh:TODO`

## Context / Why

Plans #1–#3 built the board, tabs, and Claude dispatch with question
surfacing. This is the smallest live fire-test of that loop: dispatch a real
`claude` agent on a trivial task, but force it through the question path —
it MUST ask the human exactly 3 questions via `AskUserQuestion` (one at a
time, waiting for `bb answer` each time) before finishing.

## Goal

A `hello.sh` at the repo root that prints a greeting, built by a dispatched
agent that asks 3 prescribed questions first.

Must-haves for #4:
1. `hello.sh` exists, is executable, prints `Hello, <name>!` (name comes from Q2).
2. The agent asks exactly these 3 questions, in order, one per `AskUserQuestion` call:
   - Q1: which language? options: `bash` / `python` / `rust` (answer: pick one)
   - Q2: whom should it greet? free text (answer: a name)
   - Q3: how should it verify? options: `run-it` / `just-print`
3. Each question flips the cell to `[!] blocked` with the question text; each
   `bb answer` flips it back to `[~] executing`. Finish writes `run-report` + `done`.

Non-goals: no new `bb` features, no TUI changes, no second slice.

## Slices

- #4/hello — hello world via 3 dispatched questions

Example board mid-run:

```
#4 hello-questions
  [!] hello  blocked  human  "Q: whom should it greet? — unblocks on bb answer '#4/hello' --pick ..."
  run: session abc123 | $0.010 | 3k in / 1k out (0 cache-read) | 5 turns | 40s | ok
  refs: problems/004-hello-questions.md | gh#4
```

Legend (unchanged): `[ ]` open, `[.]` planning, `[~]` executing, `[x]` done, `[!]` blocked.

## Tuple protocol (v1 + run-report, unchanged from #3)

No changes. `blocked` summaries carry the question + `unblocks on bb answer`
hint; full `questions[]` live in `.blackboard/pending/4-hello.json`;
`run-report` goes to the `reports` table.

## Agent workflow (dispatched slice)

1. Human: `bb dispatch '#4/hello' --by human --prompt "You are implementing problems/004-hello-questions.md. Before writing any code, ask Q1 (language: bash/python/rust) via AskUserQuestion and wait. Then ask Q2 (whom to greet, free text) and wait. Then ask Q3 (verify: run-it/just-print) and wait. Only then write hello.sh and finish with a 2-sentence summary. Never assume — always ask."`
2. On each `[!]`: human runs `bb answer '#4/hello' --by human --pick "<label>"` (Q1, Q3) or `--text "<name>"` (Q2).
3. At finish: loop writes `run-report` + `done`. Verify with `./hello.sh`.

## Deterministic sync (unchanged)

`bb sync` NEVER commits code. Slice IDs come from the `## Slices` list above
(`#4/hello`); hardcoded set is fallback only.

## Tech decisions (decided)

- One slice only — this problem is a dispatch smoke test, not a feature.
- `hello.sh` at repo root (not in `src/`): keeps the Rust build untouched.
- Real `claude` binary required (this is the live dogfood for #3's mock work).

## Acceptance for #4

- [ ] `bb dispatch '#4/hello'` drives `planning -> executing -> blocked -> executing -> blocked -> executing -> blocked -> executing -> done` (3 question cycles), zero code written before Q3 is answered.
- [ ] `hello.sh` is executable and prints `Hello, <name>!` with the answered name.
- [ ] `bb show '#4'` renders the `run:` line; `done` summary is <=2 sentences.
