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

/// Global slice counts across all problems: (open, closed).
pub fn tab_counts(views: &[ProblemView]) -> (usize, usize) {
    let mut open = 0;
    let mut closed = 0;
    for p in views {
        let (o, c) = partition_open_closed(&p.slices);
        open += o.len();
        closed += c.len();
    }
    (open, closed)
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
        Some(p) => Ok(p.render()),
        None => Ok(vec![format!("#{} (no slices yet)", num)]),
    }
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
                lines.extend(p.render());
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
                lines.push(format!("#{} {}", p.num, p.slug));
                for s in picked {
                    lines.push(render_slice_line(s));
                }
                lines.push(format!("  refs: {}", p.refs_line));
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
}
