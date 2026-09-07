//! `bb` — Blackboard CLI MVP (plan #1).
//!
//! Local-first: all reads hit `.blackboard/index.db`. No `git` subprocess
//! anywhere on the read or write path.
mod board;
mod claude;
mod cli;
mod dispatch;
mod help;
mod namespace;
mod store;
mod sync;
mod tui;
mod tuple;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use cli::{Cli, Command};
use store::Store;
use tuple::Tuple;

fn explain(store: &Store, ns: &namespace::Namespace) {
    println!("namespace: {} ({})", ns.root.display(), ns.origin.describe());
    println!(
        "explain: reads={} rows={} git=none",
        store.db_path().display(),
        store.row_count()
    );
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ns = namespace::resolve(&cli.repo)?;
    let store = Store::new(&ns.root);
    let cmd = cli.cmd.as_ref().unwrap_or(&Command::Help);

    match cmd {
        Command::Help => {
            for line in help::render_help(&ns) {
                println!("{line}");
            }
        }
        Command::Init => {
            store.init()?;
            println!("init: {} (empty board)", store.dir().display());
            if cli.explain {
                explain(&store, &ns);
            }
        }
        Command::Pick { id, by } => {
            let refs = default_refs(&store)?;
            let t = Tuple::new(id, "slice-state", "planning", by, "", refs);
            store.append(&t, false)?;
            println!("pick: {} planning by {}", id, by);
        }
        Command::State { id, state, by, summary } => {
            if !tuple::SLICE_STATES.contains(&state.as_str()) {
                anyhow::bail!("unknown state {state:?} (open|planning|executing|done|blocked)");
            }
            if state == "open" {
                anyhow::bail!("`open` comes only from deterministic `bb sync`");
            }
            if state == "done" {
                anyhow::bail!("use `bb done` for done (requires summary)");
            }
            let refs = default_refs(&store)?;
            let t = Tuple::new(id, "slice-state", state, by, summary, refs);
            store.append(&t, false)?;
            println!("state: {} {} by {}", id, state, by);
        }
        Command::Done { id, by, summary } => {
            if summary.trim().is_empty() {
                anyhow::bail!("`done` MUST include summary (<=2 sentences)");
            }
            let refs = default_refs(&store)?;
            let t = Tuple::new(id, "slice-state", "done", by, summary, refs);
            store.append(&t, false)?;
            println!("done: {} [x] \"{}\"", id, summary);
        }
        Command::Show { id } => {
            let num = id.trim_start_matches('#').parse::<u64>().context("show id like #1")?;
            for line in board::render_show(&store, num)? {
                println!("{line}");
            }
            if cli.explain {
                explain(&store, &ns);
            }
        }
        Command::Board { open, closed } => {
            let tab = if *open {
                Some(board::Tab::Open)
            } else if *closed {
                Some(board::Tab::Closed)
            } else {
                None
            };
            for line in board::render_board_filtered(&store, tab)? {
                println!("{line}");
            }
            if cli.explain {
                explain(&store, &ns);
            }
        }
        Command::Sync { file, offline } => {
            let n = sync::sync_file(&store, &PathBuf::from(file), *offline)?;
            println!("sync: issue #{} (frontmatter + blackboard updated, no code committed)", n);
            if cli.explain {
                explain(&store, &ns);
            }
        }
        Command::Tui { once, watch_ms, tab } => {
            let t = match tab.as_deref() {
                Some("open") => Some(board::Tab::Open),
                Some("closed") => Some(board::Tab::Closed),
                Some(other) => anyhow::bail!("unknown --tab {other:?} (open|closed)"),
                None => None,
            };
            if *once {
                println!("{}", tui::run_once(&store, t)?);
            } else {
                tui::run_interactive(&store, *watch_ms, t)?;
            }
        }
        Command::Dispatch { id, by, prompt, resume, mock_bin, allow_tools, model } => {
            dispatch::run_dispatch(
                &store,
                id,
                by,
                prompt.as_deref(),
                resume.as_deref(),
                mock_bin.as_deref(),
                allow_tools.as_deref(),
                model,
            )?;
        }
        Command::Answer { id, by, pick, text, all_json } => {
            dispatch::run_answer(
                &store,
                id,
                by,
                pick.as_deref(),
                text.as_deref(),
                all_json.as_deref(),
            )?;
            if cli.explain {
                explain(&store, &ns);
            }
        }
        Command::Log { id, lines } => {
            dispatch::run_log(&store, id, *lines)?;
            if cli.explain {
                explain(&store, &ns);
            }
        }
        Command::ClaudeHook { slice } => {
            dispatch::run_claude_hook(&store, slice)?;
        }
    }
    Ok(())
}

/// Default refs for agent assertions: reuse the latest issue-opened refs so
/// every tuple links back to the problem file + issue (hypertext, not dup).
fn default_refs(store: &Store) -> Result<Vec<String>> {
    let all = store.all_current().unwrap_or_default();
    if let Some(t) = all.iter().find(|t| t.r#type == "issue-opened") {
        return Ok(t.refs.clone());
    }
    if let Some(t) = all.first() {
        return Ok(t.refs.clone());
    }
    Ok(vec!["problems/001-blackboard-cli.md".to_string()])
}
