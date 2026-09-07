//! #3/dispatch — `bb dispatch` event loop, `bb answer`, `bb claude-hook`.
//!
//! Owns the `claude -p --output-format stream-json` child stdio, parses NDJSON
//! until the final `type:result`. Question surfacing is CLI-only: on
//! `AskUserQuestion` the loop flips the cell to `[!]` + writes the
//! `.blackboard/pending/<N>-<slice>.json` sidecar; `bb answer` supplies the
//! answer; the mock/real hook (`bb claude-hook`) reads it back as a
//! tool-result / permission response — never as a new prompt.
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::board;
use crate::claude::{self, Question};
use crate::store::Store;
use crate::tuple::{problem_of, slice_of, Tuple};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sidecar {
    pub session_id: String,
    pub tool_use_id: String,
    pub questions: Vec<Question>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<BTreeMap<String, String>>,
}

fn sanitize_slice(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

/// POSIX single-quote a value for embedding in a shell command line.
/// Hook `command` strings are run through a shell, so an unquoted slice id
/// like `#4/hello` is silently truncated at the `#` (comment start) and the
/// hook dies with "a value is required for '--slice'". Every interpolated
/// value gets this — paths can carry spaces and `$` just as easily.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Tools a dispatched slice may use without a permission round-trip.
/// Deliberately broad: the child is a headless implementer with null stdin,
/// so anything left out is not "ask the human", it is "fail the turn".
const DEFAULT_ALLOWED_TOOLS: &str = "Read,Write,Edit,Bash,Glob,Grep,TodoWrite,NotebookEdit";

pub fn pending_path(store: &Store, slice_id: &str) -> Result<PathBuf> {
    let num = problem_of(slice_id).context("slice id like #3/<slice>")?;
    let slice = slice_of(slice_id).context("slice id like #3/<slice>")?;
    Ok(store.pending_dir().join(format!("{num}-{}.json", sanitize_slice(&slice))))
}

pub fn read_sidecar(store: &Store, slice_id: &str) -> Result<Option<Sidecar>> {
    let p = pending_path(store, slice_id)?;
    if !p.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&p).context("read sidecar")?;
    Ok(Some(serde_json::from_str(&data).context("parse sidecar")?))
}

pub fn write_sidecar(store: &Store, slice_id: &str, sc: &Sidecar) -> Result<()> {
    let p = pending_path(store, slice_id)?;
    fs::create_dir_all(p.parent().unwrap()).ok();
    fs::write(&p, serde_json::to_string_pretty(sc).unwrap()).context("write sidecar")?;
    Ok(())
}

/// True once `bb answer` has stored answers for the pending question.
pub fn sidecar_has_answers(store: &Store, slice_id: &str) -> bool {
    match read_sidecar(store, slice_id) {
        Ok(Some(sc)) => sc.answers.is_some(),
        _ => false,
    }
}

pub fn log_path(store: &Store, slice_id: &str) -> Result<PathBuf> {
    let num = problem_of(slice_id).context("slice id like #3/<slice>")?;
    let slice = slice_of(slice_id).context("slice id like #3/<slice>")?;
    Ok(store.dir().join(format!("dispatch-{num}-{}.log", sanitize_slice(&slice))))
}

/// Last `n` non-empty lines of a dispatch log (for `bb log` / board surfacing).
/// Returns None when there is no log file yet.
pub fn read_log_tail(store: &Store, slice_id: &str, n: usize) -> Result<Option<Vec<String>>> {
    let p = log_path(store, slice_id)?;
    if !p.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&p).context("read dispatch log")?;
    let lines: Vec<String> = data.lines().map(|l| l.to_string()).filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return Ok(Some(vec![]));
    }
    let start = lines.len().saturating_sub(n.max(1));
    Ok(Some(lines[start..].to_vec()))
}

fn truncate_chars(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

pub(crate) fn default_refs(store: &Store) -> Result<Vec<String>> {
    let all = store.all_current().unwrap_or_default();
    if let Some(t) = all.iter().find(|t| t.r#type == "issue-opened") {
        return Ok(t.refs.clone());
    }
    if let Some(t) = all.first() {
        return Ok(t.refs.clone());
    }
    Ok(vec!["problems/001-blackboard-cli.md".to_string()])
}

/// Resume prompt: the human's answers, verbatim, as the continuation turn.
/// Re-sending `default_prompt` on `--resume` (the old behaviour) handed the
/// agent the same board render it had already seen and never mentioned that
/// a question had been answered — so it either re-asked or gave up. Returns
/// None when there is nothing answered to relay.
fn answer_prompt(store: &Store, slice_id: &str) -> Option<String> {
    let sc = read_sidecar(store, slice_id).ok().flatten()?;
    let answers = sc.answers?;
    if answers.is_empty() {
        return None;
    }
    let mut out = String::from("The human answered via `bb answer`:\n\n");
    for q in &sc.questions {
        if let Some(a) = answers.get(&q.question) {
            out.push_str(&format!("- Q: {}\n  A: {a}\n", q.question));
        }
    }
    out.push_str(
        "\nContinue the slice from where you stopped. Treat the answers above as \
         final — do not re-ask an answered question. If you need something else, \
         ask it with AskUserQuestion and stop; the loop will bring the answer back.",
    );
    Some(out)
}

fn default_prompt(store: &Store, num: u64) -> String {
    let ctx = board::render_show(store, num).unwrap_or_default().join("\n");
    format!(
        "{ctx}\n\nWorking agreement: implement the slice, use AskUserQuestion when blocked instead of assuming, finish with a 2-sentence summary."
    )
}

/// Human question for the blocked summary: first question + options hint.
fn blocked_summary(slice_id: &str, q: &claude::AskedQuestions) -> String {
    let first = &q.questions[0].question;
    let labels: Vec<String> = q.questions[0].options.iter().map(|o| o.label.clone()).collect();
    let hint = if labels.is_empty() { String::new() } else { format!(" ({})", labels.join("|")) };
    let short = truncate_chars(first, 200);
    format!("Q: {short}{hint} — unblocks on bb answer '{slice_id}' --pick \"<label>\"")
}

/// Spawn a detached `bb dispatch` for `slice_id` and return the log path.
/// The child outlives the TUI: stdio goes to
/// `.blackboard/dispatch-<N>-<slice>.log`, stdin is null. The TUI keeps
/// polling the indexer, so ticks appear live. `bb_bin` overrides the binary
/// (tests); `None` means "this same `bb` binary". `model` is passed through
/// as `bb dispatch --model` (TUI: `$BB_MODEL` or sonnet). `resume` carries a
/// session id for answer-continue (`bb dispatch --resume`); the log is
/// appended then so the question + answer stay in one file.
pub fn spawn_background_dispatch(
    store: &Store,
    slice_id: &str,
    by: &str,
    bb_bin: Option<&str>,
    model: &str,
    resume: Option<&str>,
) -> Result<PathBuf> {
    let log = log_path(store, slice_id)?;
    let log_file = if resume.is_some() {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .context("open dispatch log")?
    } else {
        fs::File::create(&log).context("create dispatch log")?
    };
    let err_file = log_file.try_clone().context("clone dispatch log")?;
    let bin: PathBuf = match bb_bin {
        Some(b) => PathBuf::from(b),
        None => std::env::current_exe().context("current exe")?,
    };
    let mut cmd = Command::new(&bin);
    cmd.arg("--repo")
        .arg(&store.root)
        .arg("dispatch")
        .arg(slice_id)
        .arg("--by")
        .arg(by)
        .arg("--model")
        .arg(model);
    if let Some(sid) = resume {
        cmd.arg("--resume").arg(sid);
    }
    cmd.stdin(Stdio::null())
        .stdout(log_file)
        .stderr(err_file)
        .spawn()
        .with_context(|| format!("spawn background dispatch for {slice_id}"))?;
    Ok(log)
}

/// Model for TUI-spawned dispatches: `$BB_MODEL`, else the cheap default.
pub fn tui_model() -> String {
    std::env::var("BB_MODEL").unwrap_or_else(|_| "sonnet".to_string())
}

/// `bb dispatch '#N/slice' --by actor [--prompt ...] [--resume SID] [--mock-bin PATH] [--allow-tools ...] [--model ...]`
pub fn run_dispatch(
    store: &Store,
    id: &str,
    by: &str,
    prompt: Option<&str>,
    resume: Option<&str>,
    mock_bin: Option<&str>,
    allow_tools: Option<&str>,
    model: &str,
) -> Result<()> {
    let num = problem_of(id).context("dispatch id like #3/<slice>")?;
    slice_of(id).context("dispatch id like #3/<slice>")?;
    let refs = default_refs(store)?;
    if resume.is_none() {
        store.append(&Tuple::new(id, "slice-state", "planning", by, "dispatching claude session", refs.clone()), false)?;
    }
    let exec_summary = match resume {
        Some(sid) => format!("resuming claude session {sid}"),
        None => "claude session starting, streaming stream-json".to_string(),
    };
    store.append(&Tuple::new(id, "slice-state", "executing", by, &exec_summary, refs.clone()), false)?;

    let prompt_text = match (prompt, resume) {
        (Some(p), _) => p.to_string(),
        (None, Some(_)) => answer_prompt(store, id)
            .unwrap_or_else(|| default_prompt(store, num)),
        (None, None) => default_prompt(store, num),
    };
    let bin = mock_bin.unwrap_or("claude");
    let mut args: Vec<String> = vec![
        "-p".into(),
        prompt_text.clone(),
        "--output-format".into(),
        "stream-json".into(),
        // Required: `claude -p --output-format stream-json` errors with
        // "requires --verbose" without it (exit 1, no type:result).
        "--verbose".into(),
    ];
    if let Some(sid) = resume {
        args.push("--resume".into());
        args.push(sid.to_string());
    }
    // Hook for real claude: same binary, no second artifact.
    // Two pieces, both required:
    // - `--permission-prompt-tool stdio` enables the AskUserQuestion tool in
    //   headless `-p` mode (without any flag the tool is hidden from the
    //   agent). `stdio` is the valid non-MCP value: permission prompts ride
    //   the stdio control channel instead of an MCP lookup. The old
    //   `--permission-prompt-tool "<shell-cmd>"` form is wrong: that flag
    //   expects an MCP tool name, so claude died with "MCP tool ... not
    //   found" + exit 1 on the first permission prompt.
    // - `--settings` PreToolUse (matcher AskUserQuestion) feeds `bb answer`
    //   back as allow+updatedInput on `--resume`. PreToolUse runs before
    //   the permission step, so an answered question never reaches stdio.
    args.push("--permission-prompt-tool".into());
    args.push("stdio".into());
    if let Ok(exe) = std::env::current_exe() {
        let hook_cmd = format!(
            "{} --repo {} claude-hook --slice {}",
            shell_quote(&exe.display().to_string()),
            shell_quote(&store.root.display().to_string()),
            shell_quote(id)
        );
        let settings = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "AskUserQuestion",
                        "hooks": [
                            { "type": "command", "command": hook_cmd }
                        ]
                    }
                ]
            }
        });
        args.push("--settings".into());
        args.push(serde_json::to_string(&settings).unwrap());
    }
    // Without this the child has no way to approve anything: stdin is null,
    // so the `stdio` permission channel can never answer a prompt and every
    // tool needing one dies with "Tool permission request failed:
    // AbortError: Stream closed" (that is what silently blocked all writes
    // in the #4/hello run). Pre-approving the work tools keeps the prompt
    // path unused; AskUserQuestion still reaches the PreToolUse hook, which
    // is where human input belongs.
    args.push("--allowedTools".into());
    args.push(allow_tools.unwrap_or(DEFAULT_ALLOWED_TOOLS).to_string());
    // Cheap default (CLI: --model, default sonnet): without this the child
    // inherits the user's default (e.g. opus), burning opus tokens per tick.
    args.push("--model".into());
    args.push(model.to_string());

    let mut child = Command::new(bin)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {bin}"))?;
    let stdout = child.stdout.take().context("capture child stdout")?;
    let reader = BufReader::new(stdout);

    let mut session_id = resume.unwrap_or("").to_string();
    let mut last_tool_use = String::new();
    let mut tool_uses: usize = 0;
    let mut final_report: Option<Tuple> = None;
    let mut final_done: Option<Tuple> = None;
    let mut final_blocked: Option<Tuple> = None;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(t) {
            Ok(v) => v,
            Err(_) => continue, // ignore non-JSON chatter
        };
        if let Some(s) = claude::session_of(&v) {
            if !s.is_empty() {
                session_id = s;
            }
        }
        tool_uses += claude::count_tool_uses(&v);
        if let Some(q) = claude::extract_questions(&v) {
            if q.tool_use_id != last_tool_use {
                last_tool_use = q.tool_use_id.clone();
                let sc = Sidecar {
                    session_id: session_id.clone(),
                    tool_use_id: q.tool_use_id.clone(),
                    questions: q.questions.clone(),
                    answers: None,
                };
                write_sidecar(store, id, &sc)?;
                let summary = blocked_summary(id, &q);
                store.append(&Tuple::new(id, "slice-state", "blocked", by, &summary, refs.clone()), false)?;
                println!("blocked: {id} [!] \"{}\"", truncate_chars(&summary, 200));
            }
            continue;
        }
        if let Some(r) = claude::parse_result_line(&v) {
            let sid = if r.session_id.is_empty() { session_id.clone() } else { r.session_id.clone() };
            let mut rr = r.clone();
            if rr.session_id.is_empty() {
                rr.session_id = sid.clone();
            }
            let summary = rr.report_summary();
            let report = Tuple::new(id, "run-report", "done", by, &summary, refs.clone())
                .with_extra(rr.extra_json());
            final_report = Some(report);
            // A question was asked in this process but never answered (the
            // headless turn ended first, e.g. stdio permission failed fast
            // with stdin null). That is blocked-awaiting-answer, never done:
            // the human answers, then resumes with --resume.
            let asked_unanswered = !last_tool_use.is_empty()
                && !sidecar_has_answers(store, id);
            if asked_unanswered {
                let qpart = match read_sidecar(store, id) {
                    Ok(Some(sc)) if !sc.questions.is_empty() => {
                        let aq = claude::AskedQuestions {
                            tool_use_id: sc.tool_use_id.clone(),
                            questions: sc.questions.clone(),
                        };
                        blocked_summary(id, &aq)
                    }
                    _ => format!("question asked, awaiting bb answer '{id}'"),
                };
                let bs = if sid.is_empty() {
                    format!("{qpart} — resume with bb dispatch '{id}' --by <you>")
                } else {
                    format!("{qpart} — resume with bb dispatch '{id}' --by <you> --resume {sid}")
                };
                final_blocked = Some(Tuple::new(id, "slice-state", "blocked", by, &bs, refs.clone()));
                break;
            }
            if rr.is_error {
                let s = truncate_chars(&rr.result, 280);
                let bs = if s.is_empty() {
                    format!("run failed ({sid}) — unblocks on re-dispatch / bb dispatch --resume {sid}")
                } else {
                    format!("run failed ({sid}): {s} — unblocks on re-dispatch / bb dispatch --resume {sid}")
                };
                final_blocked = Some(Tuple::new(id, "slice-state", "blocked", by, &bs, refs.clone()));
            } else if tool_uses == 0 {
                // Clean exit, zero tool calls: the agent gave up or stalled
                // without touching the repo. `is_error:false` says the CLI
                // ran fine, not that the slice landed — recording `done`
                // here is how #4/hello went green with no hello.sh.
                let s = truncate_chars(&rr.result, 200);
                let bs = format!(
                    "agent ended without any tool call — nothing was built: {s} — unblocks on re-dispatch / bb dispatch --resume {sid}"
                );
                final_blocked = Some(Tuple::new(id, "slice-state", "blocked", by, &bs, refs.clone()));
            } else {
                let ds = claude::first_two_sentences(&rr.result, 280);
                let ds = if ds.trim().is_empty() {
                    "Dispatch finished, verify with bb show.".to_string()
                } else {
                    ds
                };
                final_done = Some(Tuple::new(id, "slice-state", "done", by, &ds, refs.clone()));
            }
            break;
        }
    }

    let status = child.wait().context("wait for child")?;
    if let Some(rep) = final_report {
        store.append(&rep, false)?;
        if let Some(d) = final_done {
            store.append(&d, false)?;
            println!("done: {id} [x] \"{}\"", d.summary);
        } else if let Some(b) = final_blocked {
            store.append(&b, false)?;
            println!("blocked: {id} [!] \"{}\"", truncate_chars(&b.summary, 200));
        }
        if let Some(r) = store.report_for(id)? {
            if let Some(line) = board::run_line(&r) {
                println!("{line}");
            }
        }
        return Ok(());
    }

    // Child exited without a type:result — record blocked, never done.
    if !status.success() {
        let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string());
        let bs = format!("claude exited {code} without type:result (session {session_id}) — see bb log '{id}' — unblocks on re-dispatch");
        store.append(&Tuple::new(id, "slice-state", "blocked", by, &bs, refs.clone()), false)?;
        anyhow::bail!("claude exited {code} without type:result (see bb log '{id}')");
    }
    let bs = format!("stream ended without type:result (session {session_id}) — see bb log '{id}' — unblocks on re-dispatch");
    store.append(&Tuple::new(id, "slice-state", "blocked", by, &bs, refs.clone()), false)?;
    anyhow::bail!("stream ended without type:result (see bb log '{id}')");
}

/// `bb answer '#N/slice' (--pick LABEL | --text TEXT | --all-json JSON)`
pub fn run_answer(
    store: &Store,
    id: &str,
    by: &str,
    pick: Option<&str>,
    text: Option<&str>,
    all_json: Option<&str>,
) -> Result<()> {
    problem_of(id).context("answer id like #3/<slice>")?;
    slice_of(id).context("answer id like #3/<slice>")?;
    let sc = read_sidecar(store, id)?.context(format!("no pending question for {id}"))?;
    if sc.questions.is_empty() {
        anyhow::bail!("no pending question for {id}");
    }
    let mut answers: BTreeMap<String, String> = BTreeMap::new();
    if let Some(j) = all_json {
        let v: serde_json::Value = serde_json::from_str(j).context("parse --all-json")?;
        let obj = v.as_object().context("--all-json must be an object")?;
        for q in &sc.questions {
            let a = obj
                .get(&q.question)
                .and_then(|x| x.as_str())
                .with_context(|| format!("--all-json missing answer for {:?}", q.question))?;
            validate_label(q, a)?;
            answers.insert(q.question.clone(), a.to_string());
        }
    } else if let Some(p) = pick {
        if sc.questions.len() > 1 {
            anyhow::bail!("multiple questions pending — use --all-json");
        }
        answers.insert(sc.questions[0].question.clone(), p.to_string());
    } else if let Some(t) = text {
        if sc.questions.len() > 1 {
            anyhow::bail!("multiple questions pending — use --all-json");
        }
        answers.insert(sc.questions[0].question.clone(), t.to_string());
    } else {
        anyhow::bail!("answer needs --pick <label>, --text <text> or --all-json '<obj>'");
    }
    let sid = submit_answers(store, id, by, answers)?;
    println!("answer: {id} executing by {by}");
    if !sid.is_empty() {
        println!("resume with: bb dispatch '{id}' --by <you> --resume {sid}");
    }
    Ok(())
}

/// Shared answer submit behind `bb answer` AND the TUI answer mode: validate
/// every label against its question's options, store the answers in the
/// sidecar, flip the slice to `executing`. Returns the session id so the
/// caller can resume (`bb dispatch --resume`, TUI does it automatically).
pub fn submit_answers(
    store: &Store,
    id: &str,
    by: &str,
    answers: BTreeMap<String, String>,
) -> Result<String> {
    problem_of(id).context("answer id like #3/<slice>")?;
    slice_of(id).context("answer id like #3/<slice>")?;
    if answers.is_empty() {
        anyhow::bail!("no answers supplied for {id}");
    }
    let mut sc = read_sidecar(store, id)?.context(format!("no pending question for {id}"))?;
    if sc.questions.is_empty() {
        anyhow::bail!("no pending question for {id}");
    }
    for q in &sc.questions {
        let a = answers
            .get(&q.question)
            .with_context(|| format!("missing answer for {:?}", q.question))?;
        validate_label(q, a)?;
    }
    sc.answers = Some(answers);
    let sid = sc.session_id.clone();
    write_sidecar(store, id, &sc)?;
    let refs = default_refs(store)?;
    let summary = if sid.is_empty() {
        "answer supplied, resuming session".to_string()
    } else {
        format!("answer supplied, resuming session {sid}")
    };
    store.append(&Tuple::new(id, "slice-state", "executing", by, &summary, refs), false)?;
    Ok(sid)
}

fn validate_label(q: &Question, label: &str) -> Result<()> {
    if q.options.is_empty() {
        return Ok(()); // free-form question
    }
    if q.options.iter().any(|o| o.label == label) {
        return Ok(());
    }
    let valid: Vec<String> = q.options.iter().map(|o| o.label.clone()).collect();
    anyhow::bail!("unknown label {label:?} (valid: {})", valid.join(", "))
}

/// `bb log '#N/slice' [--lines N]` — print the dispatch log tail.
/// Intuitive reader for the TUI's `(log ...)` path: never asks the human
/// to remember the path or run tail themselves.
pub fn run_log(store: &Store, slice_id: &str, n: usize) -> Result<()> {
    let p = log_path(store, slice_id)?;
    if !p.exists() {
        anyhow::bail!("no dispatch log yet for {slice_id} (expected {}) — dispatch first with Enter / bb dispatch", p.display());
    }
    match read_log_tail(store, slice_id, n)? {
        None => {
            println!("log {} is empty for {slice_id}", p.display());
        }
        Some(tail) if tail.is_empty() => {
            println!("log {} is empty for {slice_id}", p.display());
        }
        Some(tail) => {
            println!("log {} (last {} lines) for {slice_id}:", p.display(), tail.len());
            for l in tail {
                println!("  {l}");
            }
        }
    }
    // Next-action hint: question pending -> answer, else re-dispatch.
    if let Ok(Some(sc)) = read_sidecar(store, slice_id) {
        if !sc.questions.is_empty() && sc.answers.is_none() {
            println!("next: bb answer '{slice_id}' --pick \"<label>\"  (question pending)");
            return Ok(());
        }
    }
    println!("next: bb dispatch '{slice_id}' --by <you>  (re-dispatch)  |  bb show '#{}'",
        problem_of(slice_id).unwrap_or(0));
    Ok(())
}

/// `bb claude-hook --slice '#N/slice'` — invoked by claude as a PreToolUse
/// hook (matcher AskUserQuestion, wired via `bb dispatch --settings`).
/// Merges the LIVE tool input (hook JSON on stdin) with the sidecar answers:
/// allow + updatedInput(incoming questions + answers) once answered, else
/// ask. Replaying the incoming input verbatim matters — the agent may
/// rephrase (descriptions differ per attempt) and claude validates the
/// rewritten call against the schema, so substituting stale sidecar
/// questions fails validation.
/// stdin is optional: with no usable hook input (e.g. the e2e mock calling
/// the hook directly) it falls back to the sidecar questions.
pub fn run_claude_hook(store: &Store, slice: &str) -> Result<()> {
    let sc = read_sidecar(store, slice)?.context(format!("no pending question for {slice}"))?;
    let incoming = read_hook_tool_input();
    // Only replay answers that belong to THIS call. The dispatch loop
    // rewrites the sidecar as soon as a new AskUserQuestion appears on the
    // stream, so a stale `answers` map can outlive the question it answered
    // — injecting it into the next, different question would silently
    // fabricate a human decision.
    let asked: Vec<String> = incoming
        .as_ref()
        .and_then(|i| i.get("questions"))
        .and_then(|q| q.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|q| q.get("question").and_then(|t| t.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_else(|| sc.questions.iter().map(|q| q.question.clone()).collect());
    let answers = sc.answers.filter(|a| {
        !asked.is_empty() && asked.iter().all(|q| a.contains_key(q))
    });
    if let Some(answers) = answers {
        let base = incoming.unwrap_or_else(|| {
            serde_json::json!({ "questions": sc.questions })
        });
        let mut updated = base;
        if let Some(obj) = updated.as_object_mut() {
            let map: std::collections::BTreeMap<String, String> = answers.into_iter().collect();
            obj.insert("answers".to_string(), serde_json::to_value(&map).unwrap());
        }
        let out = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": updated,
            }
        });
        println!("{}", serde_json::to_string(&out).unwrap());
    } else {
        // `deny`, not `ask`: with stdin null an `ask` falls through to the
        // stdio permission channel that nobody is listening on, and the
        // agent gets "AbortError: Stream closed" — an error it retries.
        // A denial with a reason tells it to stop and wait for the resume.
        let out = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason":
                    "Question recorded on the blackboard; the human answers it out of band \
                     with `bb answer`. Stop this turn now — do not retry or assume an \
                     answer. The dispatch loop will resume you with the answer.",
            }
        });
        println!("{}", serde_json::to_string(&out).unwrap());
    }
    Ok(())
}

/// Best-effort read of the PreToolUse hook input (`tool_input` object) from
/// stdin. Returns None when stdin is a terminal, empty, or unparsable —
/// callers fall back to the sidecar. Never blocks on a live pipe longer
/// than the input takes: hook stdin is one JSON object + EOF.
fn read_hook_tool_input() -> Option<serde_json::Value> {
    use std::io::{IsTerminal, Read};
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut buf = String::new();
    if stdin.lock().read_to_string(&mut buf).is_err() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(buf.trim()).ok()?;
    v.get("tool_input").cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("bb-dispatch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Store::new(&dir)
    }

    #[test]
    fn sidecar_round_trip_and_label_validation() {
        let s = tmp_store("sidecar");
        s.init().unwrap();
        let sc = Sidecar {
            session_id: "mock-1".into(),
            tool_use_id: "tu_1".into(),
            questions: vec![Question {
                question: "which hook path?".into(),
                header: None,
                options: vec![
                    crate::claude::QOption { label: "hook".into(), description: None },
                    crate::claude::QOption { label: "prompt-tool".into(), description: None },
                ],
                multi_select: Some(false),
            }],
            answers: None,
        };
        write_sidecar(&s, "#3/dispatch", &sc).unwrap();
        // bad label errors
        assert!(run_answer(&s, "#3/dispatch", "human", Some("nope"), None, None).is_err());
        // good label flips to executing + stores answers
        run_answer(&s, "#3/dispatch", "human", Some("hook"), None, None).unwrap();
        let back = read_sidecar(&s, "#3/dispatch").unwrap().unwrap();
        assert_eq!(back.answers.unwrap()["which hook path?"], "hook");
        let cur = s.all_current().unwrap();
        assert!(cur.iter().any(|t| t.id == "#3/dispatch" && t.state == "executing"));
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn pending_path_format() {
        let s = Store::new(std::path::Path::new("/tmp/x"));
        assert_eq!(
            pending_path(&s, "#3/dispatch").unwrap(),
            std::path::PathBuf::from("/tmp/x/.blackboard/pending/3-dispatch.json")
        );
    }

    fn answerable_store(name: &str) -> Store {
        let s = tmp_store(name);
        s.init().unwrap();
        let sc = Sidecar {
            session_id: "sess-9".into(),
            tool_use_id: "tu_9".into(),
            questions: vec![Question {
                question: "which color?".into(),
                header: None,
                options: vec![
                    crate::claude::QOption { label: "red".into(), description: None },
                    crate::claude::QOption { label: "blue".into(), description: None },
                ],
                multi_select: Some(false),
            }],
            answers: None,
        };
        write_sidecar(&s, "#9/probe", &sc).unwrap();
        s
    }

    #[test]
    fn submit_answers_stores_validated_answers_and_returns_session() {
        let s = answerable_store("submit-ok");
        let mut answers = BTreeMap::new();
        answers.insert("which color?".to_string(), "red".to_string());
        let sid = submit_answers(&s, "#9/probe", "tui", answers).unwrap();
        assert_eq!(sid, "sess-9");
        let back = read_sidecar(&s, "#9/probe").unwrap().unwrap();
        assert_eq!(back.answers.unwrap()["which color?"], "red");
        let cur = s.all_current().unwrap();
        assert!(cur.iter().any(|t| t.id == "#9/probe" && t.state == "executing"));
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn submit_answers_rejects_unknown_label_and_writes_nothing() {
        let s = answerable_store("submit-bad");
        let mut answers = BTreeMap::new();
        answers.insert("which color?".to_string(), "green".to_string());
        assert!(submit_answers(&s, "#9/probe", "tui", answers).is_err());
        let back = read_sidecar(&s, "#9/probe").unwrap().unwrap();
        assert!(back.answers.is_none());
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn resume_spawn_appends_log_instead_of_truncating() {
        let s = tmp_store("resumelog");
        s.init().unwrap();
        let p = log_path(&s, "#3/dispatch").unwrap();
        std::fs::write(&p, "question line\n").unwrap();
        let log = spawn_background_dispatch(&s, "#3/dispatch", "tui", Some("/bin/true"), "sonnet", Some("sess-9")).unwrap();
        assert_eq!(log, p);
        // /bin/true exits at once; give it a tick, then history must survive.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let data = std::fs::read_to_string(&p).unwrap();
        assert!(data.contains("question line"), "resume must append, got: {data:?}");
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn background_spawn_creates_log() {
        let s = tmp_store("bgspawn");
        s.init().unwrap();
        let log = spawn_background_dispatch(&s, "#3/dispatch", "tui-agent", Some("/bin/true"), "sonnet", None).unwrap();
        assert!(log.exists());
        assert_eq!(log.file_name().unwrap().to_str().unwrap(), "dispatch-3-dispatch.log");
        assert_eq!(log, log_path(&s, "#3/dispatch").unwrap());
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn shell_quote_survives_hash_and_quotes() {
        // `#4/hello` unquoted is a shell comment — the bug that killed every
        // AskUserQuestion in the #4 run.
        assert_eq!(shell_quote("#4/hello"), "'#4/hello'");
        assert_eq!(shell_quote("/a b/bb"), "'/a b/bb'");
        assert_eq!(shell_quote("it's"), r#"'it'\''s'"#);
    }

    #[test]
    fn answer_prompt_relays_answers_not_the_board() {
        let store = tmp_store("answer-prompt");
        store.init().unwrap();
        let id = "#4/hello";
        assert!(answer_prompt(&store, id).is_none(), "no sidecar -> no prompt");
        let sc = Sidecar {
            session_id: "s1".into(),
            tool_use_id: "tu1".into(),
            questions: vec![Question {
                question: "Q1: Which language?".into(),
                header: None,
                options: vec![
                    crate::claude::QOption { label: "bash".into(), description: None },
                    crate::claude::QOption { label: "rust".into(), description: None },
                ],
                multi_select: Some(false),
            }],
            answers: None,
        };
        write_sidecar(&store, id, &sc).unwrap();
        assert!(answer_prompt(&store, id).is_none(), "unanswered -> no prompt");
        let mut answers = BTreeMap::new();
        answers.insert("Q1: Which language?".to_string(), "bash".to_string());
        submit_answers(&store, id, "human", answers).unwrap();
        let p = answer_prompt(&store, id).expect("answered -> prompt");
        assert!(p.contains("Q1: Which language?"));
        assert!(p.contains("bash"));
        assert!(!p.contains("Working agreement"), "must not be the board render");
    }

    #[test]
    fn log_tail_reads_last_n() {
        let s = tmp_store("logtail");
        s.init().unwrap();
        let p = log_path(&s, "#4/hello").unwrap();
        std::fs::write(&p, "line1\n\nline2\nline3\n").unwrap();
        let tail = read_log_tail(&s, "#4/hello", 2).unwrap().unwrap();
        assert_eq!(tail, vec!["line2".to_string(), "line3".to_string()]);
        // Missing log -> None (so board stays clean when never dispatched).
        assert!(read_log_tail(&s, "#4/other", 2).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&s.root);
    }
}
