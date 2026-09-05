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
            let summary = if s.summary.trim().is_empty() && s.state == "open" {
                "waiting pick".to_string()
            } else {
                s.summary.clone()
            };
            lines.push(format!(
                "  {} {:<8} {:<8} {}  \"{}\"",
                glyph(&s.state),
                s.slice,
                s.state,
                if s.actor.is_empty() { "—" } else { &s.actor },
                summary
            ));
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

pub fn render_board(store: &Store) -> anyhow::Result<Vec<String>> {
    let all = store.all_current()?;
    let views = projection(&all);
    if views.is_empty() {
        return Ok(vec!["(empty board — run `bb sync <file>` first)".to_string()]);
    }
    let mut lines = vec!["Legend: [ ] open, [.] planning, [~] executing, [x] done, [!] blocked.".to_string()];
    for p in &views {
        lines.extend(p.render());
    }
    Ok(lines)
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
}
