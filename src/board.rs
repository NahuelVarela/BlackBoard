//! #1/board — projection shared by `bb show`, `bb board` and the TUI.
//!
//! Same function, same output. Reads come from the rusqlite indexer only.
use std::collections::BTreeMap;

use crate::store::Store;
use crate::tuple::{problem_of, slice_of, Tuple};

pub fn glyph(state: &str) -> &'static str {
    match state {
        "open" => "[ ]",
        "planning" => "[.]",
        "executing" => "[~]",
        "done" => "[x]",
        "blocked" => "[!]",
        _ => "[?]",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Open,
    Closed,
}

pub fn is_open_state(state: &str) -> bool {
    matches!(state, "open" | "planning" | "executing" | "blocked")
}

pub const LEGEND: &str = "Legend: [ ] open, [.] planning, [~] executing, [x] done, [!] blocked.";
pub const OPEN_EMPTY: &str = "(empty — everything is done)";
pub const CLOSED_EMPTY: &str = "(empty — nothing closed yet)";

/// Spinner frames for `executing` slices in the TUI. Same `[_]` width as
/// every other glyph, so lines never jitter — only the inner char moves.
pub const SPINNER_FRAMES: [&str; 4] = ["[-]", "[\\]", "[|]", "[/]"];

/// One spinner frame by tick. Wraps, so the animation loops forever.
pub fn spinner_glyph(tick: u64) -> &'static str {
    SPINNER_FRAMES[(tick as usize) % SPINNER_FRAMES.len()]
}

/// Glyph for a state at a tick. Only `executing` moves; every other state
/// returns its static glyph, so `bb board` output stays deterministic.
pub fn glyph_for(state: &str, tick: u64) -> &'static str {
    if state == "executing" {
        spinner_glyph(tick)
    } else {
        glyph(state)
    }
}

/// Partition a problem's slices into (open, closed), preserving order.
/// Open = `open|planning|executing|blocked`; Closed = `done`.
pub fn partition_open_closed(slices: &[SliceView]) -> (Vec<&SliceView>, Vec<&SliceView>) {
    let mut open = Vec::new();
    let mut closed = Vec::new();
    for s in slices {
        if is_open_state(&s.state) {
            open.push(s);
        } else {
            closed.push(s);
        }
    }
    (open, closed)
}

/// Global problem counts per tab: (open, closed).
/// Counts problems with ≥1 slice in that tab, so a mixed problem counts in
/// both. The tab bar shows issues, not subissues (slices).
pub fn tab_counts(views: &[ProblemView]) -> (usize, usize) {
    let mut open = 0;
    let mut closed = 0;
    for p in views {
        let (o, c) = partition_open_closed(&p.slices);
        if !o.is_empty() {
            open += 1;
        }
        if !c.is_empty() {
            closed += 1;
        }
    }
    (open, closed)
}

/// Problems visible in a tab (≥1 slice in that tab), preserving order.
pub fn visible_problems(views: &[ProblemView], tab: Tab) -> Vec<&ProblemView> {
    views
        .iter()
        .filter(|p| {
            let (o, c) = partition_open_closed(&p.slices);
            match tab {
                Tab::Open => !o.is_empty(),
                Tab::Closed => !c.is_empty(),
            }
        })
        .collect()
}

/// Collapsed single line for a problem in a tab, e.g. `▸ #1 foo (5 closed)`.
/// The count is the number of slices visible in that tab.
pub fn collapsed_line(p: &ProblemView, tab: Tab) -> String {
    let (o, c) = partition_open_closed(&p.slices);
    let (n, label) = match tab {
        Tab::Open => (o.len(), "open"),
        Tab::Closed => (c.len(), "closed"),
    };
    format!("▸ #{} {} ({} {})", p.num, p.slug, n, label)
}

/// Expanded lines for a problem in a tab: `▾ #N slug`, picked slice lines,
/// refs line. Mirrors the per-problem block of `render_board_filtered`.
/// Static glyphs (`[~]` for executing) — used by tests and `--once`.
#[allow(dead_code)]
pub fn expanded_lines(p: &ProblemView, tab: Tab) -> Vec<String> {
    let (o, c) = partition_open_closed(&p.slices);
    let picked: &[&SliceView] = match tab {
        Tab::Open => &o,
        Tab::Closed => &c,
    };
    let mut lines = vec![format!("▾ #{} {}", p.num, p.slug)];
    for s in picked {
        lines.push(render_slice_line(s));
    }
    lines.push(format!("  refs: {}", p.refs_line));
    lines
}

/// Animated variant for the TUI: `executing` slices show the spinner frame
/// for `tick`, everything else is identical to `expanded_lines`.
pub fn expanded_lines_animated(p: &ProblemView, tab: Tab, tick: u64) -> Vec<String> {
    let (o, c) = partition_open_closed(&p.slices);
    let picked: &[&SliceView] = match tab {
        Tab::Open => &o,
        Tab::Closed => &c,
    };
    let mut lines = vec![format!("▾ #{} {}", p.num, p.slug)];
    for s in picked {
        lines.push(render_slice_line_at(s, tick));
    }
    lines.push(format!("  refs: {}", p.refs_line));
    lines
}

fn render_slice_line(s: &SliceView) -> String {
    let summary = if s.summary.trim().is_empty() && s.state == "open" {
        "waiting pick".to_string()
    } else {
        s.summary.clone()
    };
    format!(
        "  {} {:<8} {:<8} {}  \"{}\"",
        glyph(&s.state),
        s.slice,
        s.state,
        if s.actor.is_empty() { "—" } else { &s.actor },
        summary
    )
}

fn render_slice_line_at(s: &SliceView, tick: u64) -> String {
    let summary = if s.summary.trim().is_empty() && s.state == "open" {
        "waiting pick".to_string()
    } else {
        s.summary.clone()
    };
    format!(
        "  {} {:<8} {:<8} {}  \"{}\"",
        glyph_for(&s.state, tick),
        s.slice,
        s.state,
        if s.actor.is_empty() { "—" } else { &s.actor },
        summary
    )
}

#[derive(Debug, Clone)]
pub struct SliceView {
    pub slice: String,
    pub state: String,
    pub actor: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ProblemView {
    pub num: u64,
    pub slug: String,
    pub slices: Vec<SliceView>,
    pub refs_line: String,
}

impl ProblemView {
    #[allow(dead_code)]
    pub fn render(&self) -> Vec<String> {
        let mut lines = vec![format!("#{} {}", self.num, self.slug)];
        for s in &self.slices {
            lines.push(render_slice_line(s));
        }
        lines.push(format!("  refs: {}", self.refs_line));
        lines
    }

    #[allow(dead_code)]
    pub fn render_filtered(&self, tab: Tab) -> Vec<String> {
        let (open, closed) = partition_open_closed(&self.slices);
        let picked: &[&SliceView] = match tab {
            Tab::Open => &open,
            Tab::Closed => &closed,
        };
        let mut lines = vec![format!("#{} {}", self.num, self.slug)];
        if picked.is_empty() {
            let msg = match tab {
                Tab::Open => OPEN_EMPTY,
                Tab::Closed => CLOSED_EMPTY,
            };
            lines.push(format!("  {msg}"));
        } else {
            for s in picked {
                lines.push(render_slice_line(s));
            }
        }
        lines.push(format!("  refs: {}", self.refs_line));
        lines
    }
}

/// Derive a human slug from the first `.md` ref: `001-blackboard-cli.md` -> `blackboard-cli`.
pub fn slug_from_refs(refs: &[String], num: u64) -> String {
    for r in refs {
        if r.ends_with(".md") {
            let base = r.rsplit('/').next().unwrap_or(r);
            let stem = base.strip_suffix(".md").unwrap_or(base);
            // strip leading digits + dash: "001-foo" -> "foo"
            let mut i = 0;
            for c in stem.chars() {
                if c.is_ascii_digit() {
                    i += 1;
                } else {
                    break;
                }
            }
            let rest = &stem[i..];
            let slug = rest.strip_prefix('-').unwrap_or(rest);
            if !slug.is_empty() {
                return slug.to_string();
            }
            return stem.to_string();
        }
    }
    format!("problem-{}", num)
}

/// Build problem views from indexer rows. `issue-opened` tuples supply refs;
/// `slice-state` tuples supply ticks.
pub fn projection(all: &[Tuple]) -> Vec<ProblemView> {
    let mut slices: BTreeMap<u64, BTreeMap<String, &Tuple>> = BTreeMap::new();
    let mut issues: BTreeMap<u64, &Tuple> = BTreeMap::new();
    for t in all {
        if t.r#type == "issue-opened" {
            if let Some(n) = problem_of(&t.id) {
                issues.insert(n, t);
            }
        } else if t.r#type == "slice-state" {
            if let (Some(n), Some(s)) = (problem_of(&t.id), slice_of(&t.id)) {
                slices.entry(n).or_default().insert(s, t);
            } else if let Some(n) = problem_of(&t.id) {
                // bare `#N` slice-state (shouldn't happen) — ignore
                let _ = n;
            }
        }
    }
    let mut out = Vec::new();
    for (num, map) in &slices {
        let issue = issues.get(num);
        let slug_refs: Vec<String> = issue
            .map(|t| t.refs.clone())
            .or_else(|| map.values().next().map(|t| t.refs.clone()))
            .unwrap_or_default();
        let slug = slug_from_refs(&slug_refs, *num);
        let refs_line = match issue {
            Some(t) => {
                let r = t.refs.clone();
                // append stored gh number hint if refs contain gh url
                r.join(" | ")
            }
            None => map
                .values()
                .next()
                .map(|t| t.refs.join(" | "))
                .unwrap_or_default(),
        };
        let mut sv = Vec::new();
        for (slice, t) in map {
            sv.push(SliceView {
                slice: (*slice).clone(),
                state: t.state.clone(),
                actor: if t.state == "open" && t.actor == "human" {
                    "—".to_string()
                } else {
                    t.actor.clone()
                },
                summary: t.summary.clone(),
            });
        }
        out.push(ProblemView { num: *num, slug, slices: sv, refs_line });
    }
    // problems that only have an issue-opened and no slices yet
    for (num, t) in &issues {
        if slices.contains_key(num) {
            continue;
        }
        out.push(ProblemView {
            num: *num,
            slug: slug_from_refs(&t.refs, *num),
            slices: vec![],
            refs_line: t.refs.join(" | "),
        });
    }
    out.sort_by_key(|p| p.num);
    out
}

pub fn render_show(store: &Store, num: u64) -> anyhow::Result<Vec<String>> {
    let all = store.all_current()?;
    let views = projection(&all);
    match views.into_iter().find(|p| p.num == num) {
        Some(p) => Ok(with_pending_and_runs(store, &p, &p.slices.iter().collect::<Vec<_>>())),
        None => Ok(vec![format!("#{} (no slices yet)", num)]),
    }
}

/// First dispatchable slice of a (tab-filtered) list: the first `open` one.
/// `planning|executing|blocked|done` slices are owned or finished — Enter
/// skips them so one keypress never steals or regresses another agent's slice.
pub fn first_dispatchable<'a>(picked: &[&'a SliceView]) -> Option<&'a SliceView> {
    picked.iter().find(|s| s.state == "open").copied()
}

/// First retryable slice: the first `blocked` one with NO unanswered pending
/// question (i.e. crashed: exit-1, no type:result). Blocked-with-question
/// must be answered via `bb answer`, never re-dispatched — retrying would
/// orphan the waiting session.
pub fn first_retryable<'a>(store: &Store, num: u64, picked: &[&'a SliceView]) -> Option<&'a SliceView> {
    picked.iter().find(|s| {
        if s.state != "blocked" {
            return false;
        }
        let id = format!("#{num}/{}", s.slice);
        !has_pending_question(store, &id)
    }).copied()
}

/// True when a slice has an unanswered AskUserQuestion sidecar.
pub fn has_pending_question(store: &Store, slice_id: &str) -> bool {
    match crate::dispatch::read_sidecar(store, slice_id) {
        Ok(Some(sc)) => !sc.questions.is_empty() && sc.answers.is_none(),
        _ => false,
    }
}

/// Actionable next step for one slice, shown in `bb show`/`bb board` so a
/// `[!]` never leaves the human guessing. Crash-retry states name the exact
/// retry command; question-waiting states name the exact answer command.
pub fn next_action(store: &Store, num: u64, s: &SliceView) -> Option<String> {
    let id = format!("#{num}/{}", s.slice);
    match s.state.as_str() {
        "open" => Some(format!("  next: bb dispatch '{id}' --by <you> (or Enter in TUI)")),
        "blocked" if has_pending_question(store, &id) => {
            Some(format!("  next: press a in TUI to answer '{id}' here (or bb answer '{id}' --pick \"<label>\")"))
        }
        "blocked" => {
            Some(format!("  next: bb dispatch '{id}' --by <you> (retry crashed slice, or Enter in TUI)"))
        }
        _ => None,
    }
}

/// Compact `12k` formatting for token counts.
pub fn fmt_k(n: u64) -> String {
    if n >= 1000 {
        let k = n as f64 / 1000.0;
        if (k * 10.0).fract() == 0.0 {
            format!("{}k", k as u64)
        } else {
            format!("{k:.1}k")
        }
    } else {
        format!("{n}")
    }
}

fn fmt_dur(ms: u64) -> String {
    if ms >= 1000 {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

/// `run: session abc123 | $0.042 | 12k in / 3k out (1k cache-read) | 14 turns | 92s | ok`
/// Returns None when the tuple carries no usable `extra`.
pub fn run_line(report: &Tuple) -> Option<String> {
    let e = report.extra.as_ref()?;
    let sid = e.get("session_id").and_then(|v| v.as_str()).unwrap_or("?");
    let short: String = sid.chars().take(12).collect();
    let cost = e.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let inp = e.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let out = e.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache = e.get("cache_read").and_then(|v| v.as_u64()).unwrap_or(0);
    let turns = e.get("num_turns").and_then(|v| v.as_u64()).unwrap_or(0);
    let dur = e.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    let err = e.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
    Some(format!(
        "  run: session {short} | ${cost:.3} | {} in / {} out ({} cache-read) | {turns} turns | {} | {}",
        fmt_k(inp),
        fmt_k(out),
        fmt_k(cache),
        fmt_dur(dur),
        if err { "error" } else { "ok" },
    ))
}

/// Pending-question inline for a slice id like `#3/dispatch`:
/// `  ? <first question, 120ch> (bb answer '#3/dispatch' --pick ...)`
/// Read-only; answering stays in `bb answer`.
pub fn pending_inline(store: &Store, slice_id: &str) -> Option<String> {
    let sc = crate::dispatch::read_sidecar(store, slice_id).ok()??;
    if sc.questions.is_empty() {
        return None;
    }
    let first = &sc.questions[0].question;
    let short: String = first.chars().take(120).collect();
    Some(format!("  ? {short} (bb answer '{slice_id}' --pick ...)"))
}

/// Dispatch-log tail for a slice id like `#4/hello`:
/// `  ! <last log line, 160ch> (bb log '#4/hello')`
/// Read-only; full log via `bb log`. Returns None when no log file exists.
#[allow(dead_code)]
pub fn log_tail_inline(store: &Store, slice_id: &str) -> Option<String> {
    log_tail_lines(store, slice_id, 1).pop()
}

/// Up to `n` tail lines, each as `  ! <line> (bb log ...)` on the last one.
/// Used by `bb show`/`bb board` so a crash shows root cause + wrapper
/// (e.g. `--verbose` error + `exited 1`), not just the wrapper.
pub fn log_tail_lines(store: &Store, slice_id: &str, n: usize) -> Vec<String> {
    let tail = crate::dispatch::read_log_tail(store, slice_id, n).ok().flatten().unwrap_or_default();
    if tail.is_empty() {
        return vec![];
    }
    tail.iter().enumerate().map(|(i, l)| {
        let short: String = l.chars().take(160).collect();
        if i + 1 == tail.len() {
            format!("  ! {short} (bb log '{slice_id}')")
        } else {
            format!("  ! {short}")
        }
    }).collect()
}

/// Compose header + slice lines + pending `?` lines + `run:` lines + refs.
/// `picked` selects which slices are shown (full list or one tab's).
fn with_pending_and_runs(store: &Store, p: &ProblemView, picked: &[&SliceView]) -> Vec<String> {
    let mut lines = vec![format!("#{} {}", p.num, p.slug)];
    for s in picked {
        lines.push(render_slice_line(s));
    }
    for s in picked {
        if s.state == "blocked" {
            let id = format!("#{}/{}", p.num, s.slice);
            if let Some(q) = pending_inline(store, &id) {
                lines.push(q);
            }
            // Crash / exit-1 case has no question — the log tail IS the problem.
            // Show it for every blocked slice so `bb show`/`bb board` never
            // leave the human guessing at a bare `[!]`. Last 2 lines: root
            // cause + wrapper (e.g. `--verbose` error + `exited 1`).
            for l in log_tail_lines(store, &id, 2) {
                lines.push(l);
            }
            if let Some(n) = next_action(store, p.num, s) {
                lines.push(n);
            }
        }
    }
    let reports = store.all_reports().unwrap_or_default();
    for s in picked {
        let id = format!("#{}/{}", p.num, s.slice);
        if let Some(r) = reports.iter().find(|t| t.id == id) {
            if let Some(l) = run_line(r) {
                lines.push(l);
            }
        }
    }
    lines.push(format!("  refs: {}", p.refs_line));
    lines
}

#[allow(dead_code)]
pub fn render_board(store: &Store) -> anyhow::Result<Vec<String>> {
    render_board_filtered(store, None)
}

pub fn render_board_filtered(store: &Store, tab: Option<Tab>) -> anyhow::Result<Vec<String>> {
    let all = store.all_current()?;
    let views = projection(&all);
    if views.is_empty() {
        return Ok(vec!["(empty board — run `bb sync <file>` first)".to_string()]);
    }
    match tab {
        None => {
            let mut lines = vec![LEGEND.to_string()];
            for p in &views {
                let picked: Vec<&SliceView> = p.slices.iter().collect();
                lines.extend(with_pending_and_runs(store, p, &picked));
            }
            Ok(lines)
        }
        Some(t) => {
            // Tab view: only problems with at least one slice in this tab.
            // Fully-closed problems disappear from Open; fully-open disappear from Closed.
            // Legend goes at the bottom.
            let mut lines: Vec<String> = Vec::new();
            for p in &views {
                let (open, closed) = partition_open_closed(&p.slices);
                let picked: &[&SliceView] = match t {
                    Tab::Open => &open,
                    Tab::Closed => &closed,
                };
                if picked.is_empty() {
                    continue;
                }
                lines.extend(with_pending_and_runs(store, p, picked));
            }
            if lines.is_empty() {
                let msg = match t {
                    Tab::Open => OPEN_EMPTY,
                    Tab::Closed => CLOSED_EMPTY,
                };
                lines.push(format!("  {msg}"));
            }
            lines.push(LEGEND.to_string());
            Ok(lines)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuple::Tuple;

    #[test]
    fn renders_spec_example_shape() {
        let refs = vec!["problems/001-blackboard-cli.md".to_string(), "gh://x/issues/1".to_string()];
        let mk = |slice: &str, state: &str| Tuple {
            id: format!("#1/{slice}"),
            r#type: "slice-state".into(),
            state: state.into(),
            actor: "human".into(),
            summary: "".into(),
            ts: "2026-09-05T10:00:00Z".into(),
            refs: refs.clone(),
            extra: None,
        };
        let all = vec![mk("core", "open"), mk("cli", "open")];
        let views = projection(&all);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].slug, "blackboard-cli");
        let lines = views[0].render();
        assert!(lines[0] == "#1 blackboard-cli");
        assert!(lines.iter().any(|l| l.contains("[ ]") && l.contains("core")));
        assert!(lines.iter().any(|l| l.contains("[ ]") && l.contains("cli")));
    }

    fn sv(slice: &str, state: &str) -> SliceView {
        SliceView { slice: slice.into(), state: state.into(), actor: "a".into(), summary: "".into() }
    }

    #[test]
    fn partition_blocked_is_open_done_is_closed() {
        let v = vec![
            sv("a", "open"),
            sv("b", "planning"),
            sv("c", "executing"),
            sv("d", "blocked"),
            sv("e", "done"),
        ];
        let (open, closed) = partition_open_closed(&v);
        assert_eq!(open.len(), 4);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].slice, "e");
        // order preserved
        assert_eq!(open.iter().map(|s| s.slice.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn filtered_empty_states() {
        let p = ProblemView {
            num: 1,
            slug: "x".into(),
            slices: vec![sv("a", "done")],
            refs_line: "r".into(),
        };
        let open_lines = p.render_filtered(Tab::Open);
        assert!(open_lines.iter().any(|l| l.contains(OPEN_EMPTY)));
        assert!(!open_lines.iter().any(|l| l.contains("[x]")));
        let closed_lines = p.render_filtered(Tab::Closed);
        assert!(closed_lines.iter().any(|l| l.contains("[x]")));

        let q = ProblemView {
            num: 3,
            slug: "y".into(),
            slices: vec![sv("b", "open")],
            refs_line: "r".into(),
        };
        let closed_empty = q.render_filtered(Tab::Closed);
        assert!(closed_empty.iter().any(|l| l.contains(CLOSED_EMPTY)));
    }

    #[test]
    fn tab_counts_count_problems_not_slices() {
        // 2 fully-done problems with 5 + 9 slices -> Closed (2), not (14).
        let mk = |num: u64, states: &[&str]| ProblemView {
            num,
            slug: format!("p{num}"),
            slices: states.iter().map(|s| sv("a", s)).collect(),
            refs_line: "r".into(),
        };
        let views = vec![
            mk(1, &["done", "done", "done", "done", "done"]),
            mk(2, &["done", "done", "done", "done", "done", "done", "done", "done", "done"]),
        ];
        assert_eq!(tab_counts(&views), (0, 2));
        // mixed problem counts in both tabs
        let views = vec![mk(3, &["open", "done"])];
        assert_eq!(tab_counts(&views), (1, 1));
    }

    #[test]
    fn collapsed_expanded_lines() {
        let p = ProblemView {
            num: 1,
            slug: "foo".into(),
            slices: vec![sv("a", "done"), sv("b", "done")],
            refs_line: "r".into(),
        };
        assert_eq!(collapsed_line(&p, Tab::Closed), "▸ #1 foo (2 closed)");
        let lines = expanded_lines(&p, Tab::Closed);
        assert!(lines[0].starts_with("▾ #1 foo"));
        assert!(lines.iter().any(|l| l.contains("[x]")));
        assert!(lines.iter().any(|l| l.contains("refs:")));
    }

    #[test]
    fn run_line_golden() {
        let r = Tuple {
            id: "#3/dispatch".into(),
            r#type: "run-report".into(),
            state: "done".into(),
            actor: "agent-1".into(),
            summary: "session abc123 ok".into(),
            ts: "2026-09-05T12:00:00Z".into(),
            refs: vec!["p".into()],
            extra: Some(serde_json::json!({
                "session_id": "abc123", "cost_usd": 0.042,
                "input_tokens": 12000, "output_tokens": 3000,
                "cache_read": 1000, "cache_write": 0,
                "duration_ms": 92000, "num_turns": 14,
                "is_error": false, "model": "sonnet",
            })),
        };
        let line = run_line(&r).unwrap();
        assert!(line.contains("run: session abc123"), "{line}");
        assert!(line.contains("$0.042"), "{line}");
        assert!(line.contains("14 turns"), "{line}");
        assert!(line.contains("ok"), "{line}");
        assert!(!line.contains("error"), "{line}");
    }

        #[test]
    fn first_dispatchable_skips_owned_slices() {
        let v = vec![
            sv("a", "executing"),
            sv("b", "blocked"),
            sv("c", "open"),
            sv("d", "done"),
        ];
        let refs: Vec<&SliceView> = v.iter().collect();
        assert_eq!(first_dispatchable(&refs).unwrap().slice, "c");
        let done_v = vec![sv("x", "done")];
        let done_refs: Vec<&SliceView> = done_v.iter().collect();
        assert!(first_dispatchable(&done_refs).is_none());
    }

    #[test]
    fn first_retryable_picks_crashed_blocked_only() {
        use crate::store::Store;
        let dir = std::env::temp_dir().join(format!("bb-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::new(&dir);
        s.init().unwrap();
        // Blocked with no sidecar = crashed -> retryable.
        let v = vec![sv("hello", "blocked")];
        let refs: Vec<&SliceView> = v.iter().collect();
        assert_eq!(first_retryable(&s, 4, &refs).unwrap().slice, "hello");
        // Blocked with unanswered question -> NOT retryable (must answer).
        let sc = crate::dispatch::Sidecar {
            session_id: "m".into(),
            tool_use_id: "tu".into(),
            questions: vec![crate::claude::Question {
                question: "which?".into(),
                header: None,
                options: vec![],
                multi_select: None,
            }],
            answers: None,
        };
        crate::dispatch::write_sidecar(&s, "#4/hello", &sc).unwrap();
        assert!(first_retryable(&s, 4, &refs).is_none());
        assert!(has_pending_question(&s, "#4/hello"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn projection_ignores_run_reports() {
        // (body unchanged)
        let refs = vec!["problems/003-claude-dispatch.md".to_string()];
        let tick = Tuple {
            id: "#3/dispatch".into(),
            r#type: "slice-state".into(),
            state: "executing".into(),
            actor: "agent-1".into(),
            summary: "working".into(),
            ts: "2026-09-05T10:00:00Z".into(),
            refs: refs.clone(),
            extra: None,
        };
        let rep = Tuple {
            id: "#3/dispatch".into(),
            r#type: "run-report".into(),
            state: "done".into(),
            actor: "agent-1".into(),
            summary: "session ok".into(),
            ts: "2026-09-05T11:00:00Z".into(),
            refs: refs.clone(),
            extra: Some(serde_json::json!({"session_id": "x"})),
        };
        // Even if a report leaks into the projection input, ticks are unaffected.
        let views = projection(&[tick.clone(), rep]);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].slices.len(), 1);
        assert_eq!(views[0].slices[0].state, "executing");
    }

    #[test]
    fn spinner_glyph_cycles_frames_so_executing_looks_alive() {
        let a = spinner_glyph(0);
        let b = spinner_glyph(1);
        let c = spinner_glyph(2);
        assert!(a != b, "frame 0 ({a}) must differ from frame 1 ({b}) or nothing moves");
        assert!(b != c, "frame 1 ({b}) must differ from frame 2 ({c}) or nothing moves");
        assert_eq!(spinner_glyph(4), spinner_glyph(0), "frames must wrap so the spinner loops");
    }

    #[test]
    fn glyph_for_animates_executing_but_keeps_other_states_static() {
        assert_ne!(glyph_for("executing", 0), glyph_for("executing", 1), "executing must move between ticks");
        assert_eq!(glyph_for("open", 0), glyph_for("open", 1), "open must stay [ ]");
        assert_eq!(glyph_for("done", 0), glyph_for("done", 99), "done must stay [x]");
        assert_eq!(glyph_for("blocked", 0), glyph_for("blocked", 99), "blocked must stay [!]");
    }

    #[test]
    fn expanded_lines_animated_show_moving_executing_glyph() {
        let p = ProblemView {
            num: 4,
            slug: "hello".into(),
            slices: vec![sv("hello", "executing")],
            refs_line: "r".into(),
        };
        let a = expanded_lines_animated(&p, Tab::Open, 0).join("\n");
        let b = expanded_lines_animated(&p, Tab::Open, 1).join("\n");
        assert!(a.contains("hello"), "{a}");
        assert!(b.contains("hello"), "{b}");
        assert!(a != b, "executing line must change between ticks or the TUI looks dead: {a:?} vs {b:?}");
    }
}
