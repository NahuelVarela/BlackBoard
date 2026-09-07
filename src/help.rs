//! #7/help — rich `bb help` (and bare `bb`): workflow guide + resolved
//! namespace + install self-check. `--help`/`-h`/`bb <verb> --help` stay
//! clap-standard and untouched; this is a separate, first-class render
//! path (see `Command::Help`, `disable_help_subcommand` in `cli.rs`).
use crate::namespace::{self, Namespace, Origin};

/// Render the full `bb help` guide. Pure function of `ns` + the current
/// executable path — no store access needed (must work before `bb init`).
pub fn render_help(ns: &Namespace) -> Vec<String> {
    let mut lines = vec![
        "bb — token-efficient issue-driven coordination for humans + agents".to_string(),
        "".to_string(),
    ];

    let label = match &ns.origin {
        Origin::Git(root) => root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| ns.root.display().to_string()),
        Origin::Explicit => ns
            .root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| ns.root.display().to_string()),
        Origin::Default => "(default)".to_string(),
    };
    lines.push(format!("Namespace: {}  ({})", label, ns.root.display()));

    let found_via = match &ns.origin {
        Origin::Explicit => "explicit --repo (skips discovery)".to_string(),
        Origin::Git(root) => {
            let cwd = std::env::current_dir()
                .ok()
                .and_then(|c| c.canonicalize().ok());
            match cwd {
                Some(cwd) if &cwd == root => ".git in this directory".to_string(),
                _ => ".git in an ancestor directory (you are in a subdirectory)".to_string(),
            }
        }
        Origin::Default => format!(
            "no .git in any ancestor — using the machine-wide default (set $XDG_DATA_HOME to relocate)"
        ),
    };
    lines.push(format!("  found via: {}", found_via));
    lines.push("  state:     .blackboard/ (log.jsonl + index.db)".to_string());
    lines.push("".to_string());

    lines.push("Human loop:".to_string());
    lines.push("  bb init                          create/open this repo's board".to_string());
    lines.push("  bb sync problems/<n>.md          .MD -> GitHub issue + board (never commits code)".to_string());
    lines.push("  bb board / bb tui                watch progress (no git on the read path)".to_string());
    lines.push("".to_string());

    lines.push("Agent loop:".to_string());
    lines.push("  bb show '#N'                     slices + refs, ~200 tokens".to_string());
    lines.push("  bb pick '#N/slice' --by <actor>  claim one slice".to_string());
    lines.push("  bb done '#N/slice' --by <actor> --summary \"<=2 sentences\"".to_string());
    lines.push("".to_string());

    let exe = std::env::current_exe().ok();
    let install_line = match &exe {
        Some(exe) => format!(
            "Install: {} (on PATH: {})",
            exe.display(),
            if namespace::on_path(exe) { "yes" } else { "no" }
        ),
        None => "Install: (unable to resolve current executable)".to_string(),
    };
    lines.push(install_line);
    lines.push("Run `bb <verb> --help` for flags on any command.".to_string());

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn git_namespace_names_repo_dir_and_describes_origin() {
        let ns = Namespace {
            root: PathBuf::from("/home/x/repos/BlackBoard"),
            origin: Origin::Git(PathBuf::from("/home/x/repos/BlackBoard")),
        };
        let lines = render_help(&ns);
        assert!(lines[2].starts_with("Namespace: BlackBoard  (/home/x/repos/BlackBoard)"));
        assert!(lines.iter().any(|l| l.contains("state:     .blackboard/")));
        assert!(lines.iter().any(|l| l.starts_with("Install:")));
    }

    #[test]
    fn default_namespace_labeled_and_explains_fallback() {
        let ns = Namespace {
            root: PathBuf::from("/home/x/.local/share/blackboard/default"),
            origin: Origin::Default,
        };
        let lines = render_help(&ns);
        assert!(lines[2].starts_with("Namespace: (default)"));
        assert!(lines.iter().any(|l| l.contains("no .git in any ancestor")));
    }

    #[test]
    fn explicit_repo_labeled_by_dir_name() {
        let ns = Namespace {
            root: PathBuf::from("/tmp/some-repo"),
            origin: Origin::Explicit,
        };
        let lines = render_help(&ns);
        assert!(lines[2].starts_with("Namespace: some-repo  (/tmp/some-repo)"));
        assert!(lines.iter().any(|l| l.contains("explicit --repo")));
    }
}
