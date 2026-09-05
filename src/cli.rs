//! #1/cli — `clap v4` derive definitions. Thin: dispatch lives in `main.rs`.
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "bb", version, about = "Blackboard CLI — token-efficient issue-driven coordination")]
pub struct Cli {
    /// Repo root (contains `.blackboard/`). Defaults to current dir.
    #[arg(long, global = true)]
    pub repo: Option<String>,

    /// Explain reads (prove indexer-only, no git).
    #[arg(long, global = true)]
    pub explain: bool,

    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create `.blackboard/log.jsonl` + `index.db` (idempotent).
    Init,
    /// Pick 1 slice: asserts `planning`. One slice per agent.
    Pick {
        /// Slice id, e.g. `#1/core`.
        id: String,
        /// Actor name, e.g. `agent-1`.
        #[arg(long)]
        by: String,
    },
    /// Generic transition: planning|executing|blocked.
    State {
        /// Slice id, e.g. `#1/core`.
        id: String,
        /// New state.
        state: String,
        #[arg(long)]
        by: String,
        #[arg(long, default_value = "")]
        summary: String,
    },
    /// Mark slice done with <=2-sentence summary. Positive tick. Stop.
    Done {
        /// Slice id, e.g. `#1/core`.
        id: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        summary: String,
    },
    /// Show one problem's slices + refs (<= ~40 lines for #1).
    Show {
        /// Problem id, e.g. `#1`.
        id: String,
    },
    /// Show all problems' ticks (same projection as TUI).
    Board,
    /// Deterministic sync: .MD -> GitHub issue create-or-update + blackboard asserts. NEVER commits code.
    Sync {
        /// Problem markdown file, e.g. `problems/001-blackboard-cli.md`.
        file: String,
        /// Skip `gh`, use fake issue #1 (tests / offline demo).
        #[arg(long)]
        offline: bool,
    },
    /// Watchable TUI over the same projection as `bb board`.
    Tui {
        /// Print once (same text as `bb board`) instead of interactive UI. Used in CI.
        #[arg(long)]
        once: bool,
        /// Poll interval in ms for watch mode.
        #[arg(long, default_value = "1000")]
        watch_ms: u64,
    },
}
