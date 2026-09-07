---
issue: null # filled by `bb sync` — if null, sync creates a GitHub issue; if set, sync updates it
---

# #8 — Done means green, one slice means one slice, and drill-down detail

`id: #8` | `status: open for planning` | `owner: human` | `refs: this repo, gh:TODO`

## Context / Why

Dispatched `#7/e2e` from the TUI tonight (one `Enter`, one agent). Two bugs
surfaced from watching it land, found by reading `.blackboard/log.jsonl`
directly since the TUI gave no clear signal either happened:

**Bug 1 — a `done` slice doesn't read as done.** `board::glyph()`
(`src/board.rs:9-16`) returns `[x]` for `done` same as every other glyph:
plain text, no color. `glyph_for()` (`src/board.rs:45-50`) only animates
`executing`; every other state is static *and* unstyled — the TUI's
`Style`/`Color` usage (`src/tui.rs:217-309`) colors the footer, the
selected-line header, pending questions (yellow) and log tails (red), but
the glyph inside each rendered slice line is plain `String` pushed into
`Line::from(l.clone())` with no per-state color at all. So `[x]` and `[ ]`
differ only by one character in a wall of monochrome text — a completed
slice gives no visual "yes, this landed" signal, which is exactly the
report: dispatched, solved, no green tick shown.

**Bug 2 — one dispatch, two slices closed, root cause found.** The log is
unambiguous:

```
#7/e2e      planning/executing   actor=tui-agent   (the dispatch this session launched)
#7/namespace planning -> done    actor=claude-agent (written mid-run, by the dispatched child itself)
#7/e2e      run-report + done    actor=tui-agent   summary: "Implemented `#7/namespace`: added `src/namespace.rs`..."
```

Only `#7/e2e` was dispatched. The dispatched agent closed `#7/namespace`
too — using its own shell access to call `bb` — and then closed `#7/e2e`
with a summary that is actually a description of the namespace work. Root
cause is two compounding gaps, both in `src/dispatch.rs`:

1. `default_prompt()` (`src/dispatch.rs:148-151`) builds the child's
   prompt from `board::render_show(store, num)` — the **whole problem**
   (`bb show '#7'`), which lists all four `#7` slices including the
   still-open `#7/namespace`. The "Working agreement" appended after it
   never names *which* slice id the agent owns. A capable agent reading
   "here are four open slices, implement the slice" reasonably picks up
   more than one.
2. `DEFAULT_ALLOWED_TOOLS` (`src/dispatch.rs:50`) includes unrestricted
   `Bash`, and `bb` itself is just a binary on `PATH` inside that shell —
   nothing stops the child from running `bb state '#7/namespace' ...` or
   `bb done '#7/namespace' ...` directly. There is no enforcement anywhere
   (loop-side or hook-side) that a dispatched child may only mutate the
   one slice id it was launched for.

This is the same-shaped bug as #6's fire-test, but unintentional: #6
proved slices can hand off through the board; #7/e2e proves an agent can
*also* silently reach past its assigned slice and no one notices because
nothing highlights it.

## Goal

Three must-haves, independent, all about making the board trustworthy to
watch and read at a glance.

1. **Color-coded state glyphs.** `done` renders as a green tick, `blocked`
   red/attention, `executing` keeps its live yellow spinner, `open`/
   `planning` stay neutral/dim — applied to the glyph in every rendered
   slice line (collapsed board, expanded TUI lines, `bb board` CLI output
   where the terminal supports color), not just the selection-highlight
   bar. `bb board`/`--once` piped to a file or a non-TTY must not emit raw
   ANSI (respect `NO_COLOR` / non-TTY detection — existing Rust practice,
   no new crate needed beyond what's already linked for crossterm/ratatui
   styling).
2. **Dispatch scope enforcement.** A dispatched agent for `#N/slice` must
   be unable to silently close, or change the state of, any slice other
   than `#N/slice`:
   - `default_prompt()` stops handing the child the whole problem's `bb
     show`. It states the one slice id explicitly and up front ("you own
     exactly `#N/slice`; other rows shown below are context only, do not
     modify them"), and the working agreement gains an explicit
     single-slice rule.
   - Add a real guard, not just a stronger prompt: something in the
     dispatch loop or the `bb claude-hook` path (`src/dispatch.rs`) that
     rejects/flags a `bb state`/`bb pick`/`bb done` invocation for a slice
     id other than the one the running dispatch owns, and surfaces that
     rejection back onto the board (e.g. a `blocked` tuple naming the
     out-of-scope attempt) rather than letting it through silently. Prompt
     wording alone is not sufficient — #7/e2e proves a capable agent will
     use tool access it technically has.
3. **Slice-level drill-down with progressive disclosure.** Today the TUI's
   cursor (`src/tui.rs` `cursor`/`visible_problems`) moves between
   *problems*, not slices, and `Enter` is already bound to dispatch/retry
   (`src/tui.rs:212`). Add a way to select an individual slice row inside
   the expanded problem (arrow down onto it) and open it with `Enter` into
   a detail view — for a `done`/`blocked`/`executing` slice this must not
   collide with the existing dispatch/retry-on-`Enter` behavior for `open`
   slices. The detail view shows, sourced from that slice's `run-report`
   tuple(s) (`extra` field, already recorded per #3 — `board::run_line()`
   at `src/board.rs:401-422` renders one such tuple today but only ever
   the latest one): **Total time, Total turns, Total tokens (in/out/cache),
   and Total cost**, summed across every `run-report` the slice
   accumulated (a slice resumed across `blocked` answers writes one
   `run-report` per resume — see `#5/goodbye` in the log, four of them for
   one slice — so "total" must aggregate, not just show the last one).

Non-goals for #8:
- No change to the `slice-state`/`run-report` tuple schema — this is a
  rendering + prompt-scoping + aggregation problem, not a protocol change.
- No retroactive fix/relabeling of the already-closed `#7/namespace` /
  `#7/e2e` tuples — the log stays as the historical record; #8 prevents
  recurrence.
- No sandboxing or tool removal beyond the scope guard itself (e.g. this
  is not "take Bash away from dispatched agents" — it's "the loop must
  notice and block writes outside the owned slice").
- No new interaction model beyond one drill-down level (detail view for
  one selected slice); no editing from inside the detail view.

## Slices (work items — tick independently)

| Slice | ID | State | Actor | Summary |
|-------|----|-------|-------|---------|
| Color-coded state glyphs (done=green, blocked=red, executing=spinner, open/planning=neutral) across TUI + `bb board` | #8/glyph-color | [ ] open | — | — |
| Dispatch prompt scoped to one slice id + explicit single-slice rule | #8/prompt-scope | [ ] open | — | — |
| Scope guard rejecting out-of-scope `bb state`/`pick`/`done` from a running dispatch | #8/scope-guard | [ ] open | — | — |
| Slice-level cursor + Enter drill-down detail view (progressive disclosure) | #8/drilldown | [ ] open | — | — |
| Total time/turns/tokens/cost aggregation across a slice's `run-report` tuples | #8/totals | [ ] open | — | — |
| E2E: color respects NO_COLOR/non-TTY, scope guard blocks a cross-slice write fixture, drill-down totals match a multi-resume fixture | #8/e2e | [ ] open | — | — |

## Agent workflow (dispatched slice)

Standard #3 dispatch workflow applies, with the #8/prompt-scope +
#8/scope-guard fix as dogfood: once those two slices are `[x] done`, every
*subsequent* dispatch in this session (including the remaining #8 slices)
should already be running under the new single-slice enforcement. If a
later #8 slice's dispatched agent still reaches past its own slice id
after #8/scope-guard lands, that is itself evidence #8/scope-guard did not
work — flag it, don't quietly work around it.

## Acceptance for #8

- [ ] `bb board`/TUI render `done` glyphs in green, `blocked` in red,
  `executing` keeps the live spinner, `open`/`planning` stay neutral —
  visible without selecting/expanding a row.
- [ ] `bb board > file.txt` (non-TTY) or `NO_COLOR=1 bb board` contains no
  ANSI escape codes.
- [ ] A dispatch fixture where the mocked/live agent attempts `bb state`
  or `bb done` on a slice id other than the one it was dispatched for is
  rejected (or blocked + surfaced on the board), and does not leave a
  `done`/`slice-state` tuple for the out-of-scope slice.
- [ ] `default_prompt()` output names the owned slice id explicitly and
  does not imply other open slices in the same problem are pickable by
  this dispatch.
- [ ] TUI: arrow-down inside an expanded problem can land on an individual
  slice row (not just the problem); `Enter` on that row opens a detail
  view showing Total time/turns/tokens/cost, distinct from the existing
  dispatch/retry `Enter` behavior on open/blocked slices.
- [ ] A multi-resume fixture (slice with >=3 `run-report` tuples, e.g.
  modeled on the real `#5/goodbye` log) shows totals equal to the sum
  across all of that slice's `run-report`s, not just the last one.
- [ ] `cargo test` + `cargo build --release` green; existing
  `tests/e2e_tabs.sh` and `tests/e2e_dispatch.sh` still pass unchanged;
  new e2e coverage per #8/e2e.

## For the planner (next agent)

Input: this file only (+ #3's plan for dispatch-loop/tuple context, not as
input). Output: `docs/plans/yyyy-mm-dd-008-plan.md` with task breakdown per
slice, file layout deltas (`src/board.rs` glyph coloring + totals
aggregation, `src/tui.rs` slice-level cursor + detail view + colored line
rendering, `src/dispatch.rs` `default_prompt()` rewrite + scope-guard
mechanism, likely touching `src/claude.rs`'s hook/permission path too),
test plan (unit: colored-vs-plain glyph output under `NO_COLOR`, totals
aggregation over synthetic multi-`run-report` fixtures, scope-guard
rejecting a synthetic out-of-scope `bb state` call; e2e: TUI drill-down
keypress sequence, cross-slice write fixture from #8/scope-guard). Decide
and document the exact scope-guard mechanism (loop-side allowlist check
before executing/permitting the child's `bb` calls, vs. hook-side
`PreToolUse` interception, vs. both) — this is the one open design
question the planner must resolve, everything else above is decided.
Commit plan before implementing (per Talwrn DevApproach).
