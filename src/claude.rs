//! #3/claude — stream-json types + tolerant parsers for the dispatch loop.
//!
//! No subprocess here; this module only interprets NDJSON lines emitted by
//! `claude -p --output-format stream-json`. All parsing is tolerant: missing
//! cache keys -> 0, either cost key accepted.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QOption {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Question {
    pub question: String,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub options: Vec<QOption>,
    #[serde(default)]
    pub multi_select: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AskedQuestions {
    pub tool_use_id: String,
    pub questions: Vec<Question>,
}

fn as_questions(v: &serde_json::Value) -> Option<Vec<Question>> {
    let arr = v.as_array()?;
    let mut out = Vec::new();
    for q in arr {
        let question = q.get("question")?.as_str()?.to_string();
        let header = q.get("header").and_then(|h| h.as_str()).map(|s| s.to_string());
        let mut options = Vec::new();
        if let Some(opts) = q.get("options").and_then(|o| o.as_array()) {
            for o in opts {
                let label = o.get("label")?.as_str()?.to_string();
                let description = o
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(|s| s.to_string());
                options.push(QOption { label, description });
            }
        }
        let multi_select = q
            .get("multiSelect")
            .and_then(|b| b.as_bool())
            .or_else(|| q.get("multi_select").and_then(|b| b.as_bool()));
        out.push(Question { question, header, options, multi_select });
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// Recursively search a stream-json line for an AskUserQuestion tool_use.
/// Returns the tool-use id + questions on the first hit.
pub fn extract_questions(line: &serde_json::Value) -> Option<AskedQuestions> {
    fn walk(v: &serde_json::Value, out: &mut Option<AskedQuestions>) {
        if out.is_some() {
            return;
        }
        if let Some(obj) = v.as_object() {
            let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name == "AskUserQuestion" {
                let input = obj.get("input");
                let qs = input
                    .and_then(|i| i.get("questions"))
                    .and_then(as_questions);
                if let Some(questions) = qs {
                    let tool_use_id = obj
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("tool_use_1")
                        .to_string();
                    *out = Some(AskedQuestions { tool_use_id, questions });
                    return;
                }
            }
            for child in obj.values() {
                walk(child, out);
            }
        } else if let Some(arr) = v.as_array() {
            for child in arr {
                walk(child, out);
            }
        }
    }
    let mut found = None;
    walk(line, &mut found);
    found
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinalResult {
    pub result: String,
    pub session_id: String,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub duration_ms: u64,
    pub num_turns: u32,
    pub is_error: bool,
    pub model: Option<String>,
}

fn u64_of(v: Option<&serde_json::Value>) -> u64 {
    v.and_then(|x| x.as_u64()).unwrap_or(0)
}

fn f64_of(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64())
}

/// Parse a final `{"type":"result",...}` line. Returns None for non-result lines.
pub fn parse_result_line(line: &serde_json::Value) -> Option<FinalResult> {
    if line.get("type")?.as_str()? != "result" {
        return None;
    }
    let result = line
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = line
        .get("session_id")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let cost_usd = f64_of(line.get("total_cost_usd"))
        .or_else(|| f64_of(line.get("cost_usd")))
        .unwrap_or(0.0);
    let usage = line.get("usage");
    let input_tokens = u64_of(usage.and_then(|u| u.get("input_tokens")));
    let output_tokens = u64_of(usage.and_then(|u| u.get("output_tokens")));
    let cache_read = u64_of(usage.and_then(|u| u.get("cache_read_input_tokens")));
    let cache_write = u64_of(usage.and_then(|u| u.get("cache_creation_input_tokens")));
    let duration_ms = u64_of(line.get("duration_ms"))
        .max(u64_of(line.get("durationMs")))
        .max(
            line.get("duration")
                .and_then(|d| d.as_f64())
                .map(|s| (s * 1000.0) as u64)
                .unwrap_or(0),
        );
    let num_turns = line
        .get("num_turns")
        .and_then(|n| n.as_u64())
        .unwrap_or(0) as u32;
    let is_error = line.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false);
    let model = line
        .get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    Some(FinalResult {
        result,
        session_id,
        cost_usd,
        input_tokens,
        output_tokens,
        cache_read,
        cache_write,
        duration_ms,
        num_turns,
        is_error,
        model,
    })
}

/// Count `tool_use` blocks in a stream-json line. A whole run with zero of
/// them means the agent talked and exited without touching the repo — never
/// a finished slice, however confident the final text sounds.
pub fn count_tool_uses(line: &serde_json::Value) -> usize {
    fn walk(v: &serde_json::Value, n: &mut usize) {
        match v {
            serde_json::Value::Object(obj) => {
                if obj.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    *n += 1;
                }
                for child in obj.values() {
                    walk(child, n);
                }
            }
            serde_json::Value::Array(arr) => {
                for child in arr {
                    walk(child, n);
                }
            }
            _ => {}
        }
    }
    let mut n = 0;
    walk(line, &mut n);
    n
}

pub fn session_of(line: &serde_json::Value) -> Option<String> {
    for key in ["session_id", "sessionId"] {
        if let Some(s) = line.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

impl FinalResult {
    pub fn extra_json(&self) -> serde_json::Value {
        serde_json::json!({
            "session_id": self.session_id,
            "cost_usd": self.cost_usd,
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "cache_read": self.cache_read,
            "cache_write": self.cache_write,
            "duration_ms": self.duration_ms,
            "num_turns": self.num_turns,
            "is_error": self.is_error,
            "model": self.model,
        })
    }

    pub fn report_summary(&self) -> String {
        format!(
            "session {} {}, {} turns, ${:.3}",
            if self.session_id.is_empty() { "?" } else { &self.session_id },
            if self.is_error { "error" } else { "ok" },
            self.num_turns,
            self.cost_usd,
        )
    }
}

/// First ~2 sentences of free text (sentence-split on `.?!` + cap).
pub fn first_two_sentences(s: &str, cap: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    for ch in s.chars() {
        out.push(ch);
        if matches!(ch, '.' | '?' | '!') {
            count += 1;
            if count >= 2 {
                break;
            }
        }
        if out.len() >= cap {
            break;
        }
    }
    let t = out.trim();
    if t.is_empty() {
        // fallback: truncate raw
        let mut r: String = s.trim().chars().take(cap).collect();
        if s.trim().len() > cap {
            r.push('…');
        }
        return r;
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ask_user_question() {
        let line: serde_json::Value = serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "text", "text": "need input"},
                {"type": "tool_use", "id": "tu_1", "name": "AskUserQuestion",
                 "input": {"questions": [
                    {"question": "which hook path?", "header": "Hook",
                     "options": [{"label": "hook"}, {"label": "prompt-tool"}],
                     "multiSelect": false}
                 ]}}
            ]}
        });
        let q = extract_questions(&line).expect("must detect");
        assert_eq!(q.tool_use_id, "tu_1");
        assert_eq!(q.questions.len(), 1);
        assert_eq!(q.questions[0].options.len(), 2);
    }

    #[test]
    fn ignores_other_tool_use() {
        let line: serde_json::Value = serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "tool_use", "id": "x", "name": "Bash", "input": {"command": "ls"}}
            ]}
        });
        assert!(extract_questions(&line).is_none());
    }

    #[test]
    fn parses_result_both_cost_keys() {
        let a: serde_json::Value = serde_json::json!({
            "type": "result", "result": "Did X. Verified with tests.",
            "session_id": "abc123", "total_cost_usd": 0.042,
            "usage": {"input_tokens": 12000, "output_tokens": 3000,
                      "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 0},
            "duration_ms": 92000, "num_turns": 14, "is_error": false, "model": "sonnet"
        });
        let r = parse_result_line(&a).unwrap();
        assert_eq!(r.session_id, "abc123");
        assert!((r.cost_usd - 0.042).abs() < 1e-9);
        assert_eq!(r.cache_read, 1000);
        assert!(!r.is_error);
        // legacy cost_usd key + missing cache keys
        let b: serde_json::Value = serde_json::json!({
            "type": "result", "result": "boom",
            "session_id": "s2", "cost_usd": 0.01,
            "usage": {"input_tokens": 5, "output_tokens": 6},
            "num_turns": 2, "is_error": true
        });
        let r2 = parse_result_line(&b).unwrap();
        assert!((r2.cost_usd - 0.01).abs() < 1e-9);
        assert_eq!(r2.cache_read, 0);
        assert!(r2.is_error);
    }

    #[test]
    fn counts_tool_uses() {
        let line: serde_json::Value = serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "text", "text": "hi"},
                {"type": "tool_use", "id": "a", "name": "Bash", "input": {}},
                {"type": "tool_use", "id": "b", "name": "Edit", "input": {}}
            ]}
        });
        assert_eq!(count_tool_uses(&line), 2);
        let talk: serde_json::Value = serde_json::json!({
            "type": "assistant", "message": {"content": [{"type": "text", "text": "giving up"}]}
        });
        assert_eq!(count_tool_uses(&talk), 0);
    }

    #[test]
    fn non_result_is_none() {
        let line: serde_json::Value = serde_json::json!({"type": "assistant"});
        assert!(parse_result_line(&line).is_none());
    }

    #[test]
    fn two_sentence_split() {
        assert_eq!(first_two_sentences("Did X. Verified with Y. Extra.", 280), "Did X. Verified with Y.");
        assert_eq!(first_two_sentences("single", 280), "single");
    }
}
