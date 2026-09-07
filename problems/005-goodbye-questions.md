---
issue: 3
---

# #5 — Goodbye world that asks 3 questions

`id: #5` | `status: open for planning` | `owner: human` | `refs: this repo, gh:TODO`

## Context / Why

#4 proved the dispatch -> question -> answer -> summary loop end to end
(`hello.sh`, done today). This is a second, independent smoke test of the
same loop — same shape, different script and questions — so a human can
exercise `bb dispatch` / `bb answer` fresh without reusing an already-`done`
slice.

## Goal

A `goodbye.sh` at the repo root that prints a farewell, built by a
dispatched agent that asks 3 prescribed questions first.

Must-haves for #5:
1. `goodbye.sh` exists, is executable, prints `Goodbye, <name>!` (name comes from Q2).
2. The agent asks exactly these 3 questions, in order, one per `AskUserQuestion` call:
   - Q1: which language? options: `bash` / `python` / `rust` (answer: pick one)
   - Q2: whom should it bid farewell to? free text (answer: a name)
   - Q3: how should it verify? options: `run-it` / `just-print`
3. Each question flips the cell to `[!] blocked` with the question text; each
   `bb answer` flips it back to `[~] executing`. Finish writes `run-report` + `done`.

Non-goals: no new `bb` features, no TUI changes, no second slice.

## Slices

- #5/goodbye — goodbye world via 3 dispatched questions

Example board mid-run:

```
#5 goodbye-questions
  [!] goodbye  blocked  human  "Q: whom should it bid farewell to? — unblocks on bb answer '#5/goodbye' --pick ..."
  run: session abc123 | $0.010 | 3k in / 1k out (0 cache-read) | 5 turns | 40s | ok
  refs: problems/005-goodbye-questions.md | gh#5
```

Legend (unchanged): `[ ]` open, `[.]` planning, `[~]` executing, `[x]` done, `[!]` blocked.

## Tuple protocol (v1 + run-report, unchanged from #3)

No changes. `blocked` summaries carry the question + `unblocks on bb answer`
hint; full `questions[]` live in `.blackboard/pending/5-goodbye.json`;
`run-report` goes to the `reports` table.

## Agent workflow (dispatched slice)

1. Human: `bb dispatch '#5/goodbye' --by human --prompt "You are implementing problems/005-goodbye-questions.md. Before writing any code, ask Q1 (language: bash/python/rust) via AskUserQuestion and wait. Then ask Q2 (whom to bid farewell to, free text) and wait. Then ask Q3 (verify: run-it/just-print) and wait. Only then write goodbye.sh and finish with a 2-sentence summary. Never assume — always ask."`
2. On each `[!]`: human runs `bb answer '#5/goodbye' --by human --pick "<label>"` (Q1, Q3) or `--text "<name>"` (Q2).
3. At finish: loop writes `run-report` + `done`. Verify with `./goodbye.sh`.

## Deterministic sync (unchanged)

`bb sync` NEVER commits code. Slice IDs come from the `## Slices` list above
(`#5/goodbye`); hardcoded set is fallback only.

## Tech decisions (decided)

- One slice only — this problem is a dispatch smoke test, not a feature.
- `goodbye.sh` at repo root (not in `src/`): keeps the Rust build untouched.
- Real `claude` binary required (dogfood, same as #4).

## Acceptance for #5

- [ ] `bb dispatch '#5/goodbye'` drives `planning -> executing -> blocked -> executing -> blocked -> executing -> blocked -> executing -> done` (3 question cycles), zero code written before Q3 is answered.
- [ ] `goodbye.sh` is executable and prints `Goodbye, <name>!` with the answered name.
- [ ] `bb show '#5'` renders the `run:` line; `done` summary is <=2 sentences.
