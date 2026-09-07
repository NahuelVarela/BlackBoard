//! #1/cli — `clap v4` derive definitions. Thin: dispatch lives in `main.rs`.
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "bb",
    version,
    about = "Blackboard CLI — token-efficient issue-driven coordination",
    after_help = "Run 'bb help' for the full guide.",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Repo root (contains `.blackboard/`). Defaults to nearest git root.
    #[arg(long, global = true)]
    pub repo: Option<String>,

    /// Explain reads (prove indexer-only, no git).
    #[arg(long, global = true)]
    pub explain: bool,

    #[command(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Full usage guide: workflow, verbs, and your current namespace.
    Help,
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
    Board {
        /// Show only open slices (open|planning|executing|blocked).
        #[arg(long, conflicts_with = "closed")]
        open: bool,
        /// Show only closed (done) slices.
        #[arg(long, conflicts_with = "open")]
        closed: bool,
    },
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
        /// Narrow `--once` (or initial tab) to one tab: open|closed.
        #[arg(long, value_parser = ["open", "closed"])]
        tab: Option<String>,
    },
    /// Dispatch a Claude Code agent on one slice; board ticks itself.
    Dispatch {
        /// Slice id, e.g. `#3/dispatch`.
        id: String,
        /// Actor name, e.g. `agent-1`.
        #[arg(long)]
        by: String,
        /// Prompt for the agent. Defaults to `bb show` slice context.
        #[arg(long)]
        prompt: Option<String>,
        /// Resume a previous session instead of starting a new one.
        #[arg(long)]
        resume: Option<String>,
        /// Override the `claude` binary (tests: path to a mock shim).
        #[arg(long)]
        mock_bin: Option<String>,
        /// Passthrough for `claude --allowedTools`.
        #[arg(long)]
        allow_tools: Option<String>,
        /// Model for the agent (passthrough for `claude --model`).
        /// Cheap default: dispatches run on sonnet unless overridden.
        #[arg(long, default_value = "sonnet")]
        model: String,
    },
    /// Answer a pending AskUserQuestion for a dispatched slice.
    Answer {
        /// Slice id, e.g. `#3/dispatch`.
        id: String,
        /// Actor name answering.
        #[arg(long)]
        by: String,
        /// Option label for the (single) pending question.
        #[arg(long, conflicts_with_all = ["text", "all_json"])]
        pick: Option<String>,
        /// Free-text answer for the (single) pending question.
        #[arg(long, conflicts_with_all = ["pick", "all_json"])]
        text: Option<String>,
        /// JSON object mapping question text -> label (multi-question).
        #[arg(long, conflicts_with_all = ["pick", "text"])]
        all_json: Option<String>,
    },
    /// Show the dispatch log tail for one slice (e.g. `#4/hello`).
    /// Reads `.blackboard/dispatch-<N>-<slice>.log` — the same file the TUI points at.
    Log {
        /// Slice id, e.g. `#4/hello`.
        id: String,
        /// How many trailing lines to show.
        #[arg(long, default_value = "20")]
        lines: usize,
    },
    /// Hook helper invoked by claude as a PreToolUse hook
    /// (matcher AskUserQuestion, via `bb dispatch --settings`). Prints
    /// the hook decision JSON (allow + updatedInput once answered).
    #[command(hide = true)]
    ClaudeHook {
        /// Slice id the question belongs to, e.g. `#3/dispatch`.
        #[arg(long)]
        slice: String,
    },
}
