//! #1/core — tuple envelope, validation, current-state reduction.
//!
//! Envelope (spec §Tuple protocol):
//! {"id":"#1/core","type":"slice-state","state":"...","actor":"...","summary":"...","ts":"RFC3339","refs":[...]}
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

pub const SLICE_STATES: &[&str] = &["open", "planning", "executing", "done", "blocked"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tuple {
    pub id: String,
    pub r#type: String,
    pub state: String,
    pub actor: String,
    pub summary: String,
    pub ts: String,
    pub refs: Vec<String>,
}

impl Tuple {
    pub fn new(
        id: &str,
        r#type: &str,
        state: &str,
        actor: &str,
        summary: &str,
        refs: Vec<String>,
    ) -> Self {
        let ts = Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
        Self {
            id: id.to_string(),
            r#type: r#type.to_string(),
            state: state.to_string(),
            actor: actor.to_string(),
            summary: summary.to_string(),
            ts,
            refs,
        }
    }

    /// Validate per spec rules. `allow_open=false` rejects `open` (agents may
    /// assert planning|executing|blocked|done only; `open` comes from `bb sync`).
    pub fn validate(&self, allow_open: bool) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("id must not be empty".into());
        }
        if self.r#type != "slice-state" && self.r#type != "issue-opened" {
            return Err(format!("unknown type {:?}", self.r#type));
        }
        if self.r#type == "slice-state" && !SLICE_STATES.contains(&self.state.as_str()) {
            return Err(format!("unknown state {:?}", self.state));
        }
        if self.actor.is_empty() {
            return Err("actor must not be empty".into());
        }
        if self.ts.parse::<DateTime<Utc>>().is_err() {
            return Err(format!("ts must be RFC3339, got {:?}", self.ts));
        }
        if self.r#type == "slice-state" {
            if self.state == "open" && !allow_open {
                return Err("`open` comes only from deterministic `bb sync`".into());
            }
            if self.state == "done" && self.summary.trim().is_empty() {
                return Err("`done` MUST include summary (1-2 sentences)".into());
            }
            if self.state == "blocked" && self.summary.trim().is_empty() {
                return Err("`blocked` MUST include reason + what unblocks in summary".into());
            }
            if self.refs.is_empty() {
                return Err("refs MUST link back to the problem file / issue".into());
            }
        }
        Ok(())
    }

    /// Ordering key: (ts, actor). Greater = newer. Tiebreak: lexicographically
    /// greater actor wins — deterministic on every replica.
    pub fn sort_key(&self) -> (DateTime<Utc>, &str) {
        let ts = self
            .ts
            .parse::<DateTime<Utc>>()
            .unwrap_or(DateTime::<Utc>::MIN_UTC);
        (ts, self.actor.as_str())
    }
}

/// Current state = tuple with max ts, tiebreak by actor lexicographically.
/// Every replica computes identically.
pub fn current<'a>(tuples: impl IntoIterator<Item = &'a Tuple>) -> Option<&'a Tuple> {
    tuples.into_iter().max_by(|a, b| {
        a.sort_key()
            .0
            .cmp(&b.sort_key().0)
            .then_with(|| a.actor.cmp(&b.actor))
    })
}

/// Problem number from id `#N` or `#N/slice`. Returns N.
pub fn problem_of(id: &str) -> Option<u64> {
    let s = id.strip_prefix('#')?;
    let num: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        return None;
    }
    num.parse().ok()
}

/// Slice name from id `#N/<slice>`, or None for bare `#N`.
pub fn slice_of(id: &str) -> Option<String> {
    let s = id.strip_prefix('#')?;
    let mut parts = s.splitn(2, '/');
    parts.next()?;
    parts.next().map(|x| x.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, state: &str, actor: &str, ts: &str, summary: &str) -> Tuple {
        Tuple {
            id: id.into(),
            r#type: "slice-state".into(),
            state: state.into(),
            actor: actor.into(),
            summary: summary.into(),
            ts: ts.into(),
            refs: vec!["problems/001-blackboard-cli.md".into()],
        }
    }

    #[test]
    fn done_requires_summary() {
        let bad = t("#1/core", "done", "a1", "2026-09-05T10:00:00Z", "");
        assert!(bad.validate(false).is_err());
        let ok = t("#1/core", "done", "a1", "2026-09-05T10:00:00Z", "JSONL log added, verify with cargo test.");
        assert!(ok.validate(false).is_ok());
    }

    #[test]
    fn open_rejected_for_agents() {
        let open = t("#1/core", "open", "a1", "2026-09-05T10:00:00Z", "");
        assert!(open.validate(false).is_err());
        assert!(open.validate(true).is_ok());
    }

    #[test]
    fn current_picks_max_ts_tiebreak_actor() {
        let a = t("#1/core", "executing", "agent-1", "2026-09-05T10:00:00Z", "working");
        let b = t("#1/core", "planning", "agent-2", "2026-09-05T10:00:00Z", "planning");
        // same ts -> lexicographically greater actor wins (agent-2)
        assert_eq!(current([&a, &b]).unwrap().actor, "agent-2");
        let c = t("#1/core", "done", "agent-1", "2026-09-05T11:00:00Z", "All done, verify with bb board.");
        assert_eq!(current([&a, &b, &c]).unwrap().state, "done");
    }

    #[test]
    fn id_parsing() {
        assert_eq!(problem_of("#1/core"), Some(1));
        assert_eq!(problem_of("#12"), Some(12));
        assert_eq!(problem_of("nope"), None);
        assert_eq!(slice_of("#1/core"), Some("core".into()));
        assert_eq!(slice_of("#1"), None);
    }
}
