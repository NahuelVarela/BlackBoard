//! #1/sync — deterministic `.MD` -> GitHub Issue create-or-update + blackboard asserts.
//!
//! NEVER runs `git add/commit/push`. Only external transport is `gh issue
//! create` / `gh issue edit`. `--offline` skips `gh` (tests).
use std::fs;
use std::path::Path;
use std::process::Command as Proc;

use anyhow::{Context, Result};

use crate::store::Store;
use crate::tuple::Tuple;

pub const SYNC_SLICES: &[&str] = &["core", "cli", "board", "tui", "sync"];

#[derive(Debug, Clone)]
pub struct Frontmatter {
    pub issue: Option<u64>,
}

pub fn parse_frontmatter(text: &str) -> Result<Frontmatter> {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if first.trim() != "---" {
        anyhow::bail!("missing frontmatter: file must start with `---`");
    }
    let mut header_lines: Vec<String> = vec!["---".to_string()];
    let mut issue: Option<u64> = None;
    let mut closed = false;
    for line in lines.by_ref() {
        header_lines.push(line.to_string());
        if line.trim() == "---" {
            closed = true;
            break;
        }
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("issue:") {
            // Strip trailing ` # comment`, then parse. Accepts `12`, `#12`, `null`, `~`, empty.
            let mut v = rest.trim();
            if let Some((before, _)) = v.split_once(" #") {
                v = before.trim();
            }
            // A value like `#12 # comment` has no leading space before first #.
            v = v.trim_start_matches('#').trim();
            // Re-strip a comment that followed a bare number: `12 # ...` handled above,
            // but `12#...` (no space) — take leading digits anyway below.
            if !v.is_empty() && v != "null" && v != "~" {
                let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    issue = digits.parse().ok();
                }
            }
        }
    }
    if !closed {
        anyhow::bail!("unterminated frontmatter (missing closing `---`)");
    }
    Ok(Frontmatter { issue })
}

pub fn set_frontmatter_issue(text: &str, n: u64) -> Result<String> {
    let fm = parse_frontmatter(text)?;
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    let mut in_header = false;
    let mut first = true;
    for line in text.lines() {
        if first && line.trim() == "---" {
            in_header = true;
            first = false;
            out.push(line.to_string());
            continue;
        }
        first = false;
        if in_header {
            if line.trim() == "---" {
                in_header = false;
                out.push(line.to_string());
                continue;
            }
            if line.trim_start().starts_with("issue:") && !replaced {
                out.push(format!("issue: {n}"));
                replaced = true;
                continue;
            }
            out.push(line.to_string());
        } else {
            out.push(line.to_string());
        }
    }
    let _ = fm;
    if !replaced {
        anyhow::bail!("frontmatter has no `issue:` field to update");
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    Ok(s)
}

fn title_of(text: &str) -> String {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# ") {
            if !rest.trim().is_empty() {
                return rest.trim().to_string();
            }
        }
    }
    "Blackboard plan".to_string()
}

fn problem_num_from_filename(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.trim_start_matches('0').parse().ok().or_else(|| digits.parse().ok())
}

/// Deterministic sync. Returns the GitHub issue number.
pub fn sync_file(store: &Store, md_path: &Path, offline: bool) -> Result<u64> {
    let abs = if md_path.is_absolute() {
        md_path.to_path_buf()
    } else {
        store.root.join(md_path)
    };
    let text = fs::read_to_string(&abs)
        .with_context(|| format!("read {}", abs.display()))?;
    let fm = parse_frontmatter(&text)?;
    let title = title_of(&text);
    let num = problem_num_from_filename(&abs).unwrap_or(1);

    let n: u64 = match fm.issue {
        Some(existing) => {
            if !offline {
                let tmp = store.dir().join("sync-body.md");
                fs::create_dir_all(store.dir()).ok();
                fs::write(&tmp, &text).context("write tmp body")?;
                let st = Proc::new("gh")
                    .args(["issue", "edit", &existing.to_string(), "--title", &title, "--body-file"])
                    .arg(&tmp)
                    .status()
                    .context("run `gh issue edit`")?;
                if !st.success() {
                    anyhow::bail!("`gh issue edit {existing}` failed");
                }
            }
            existing
        }
        None => {
            if offline {
                1
            } else {
                let tmp = store.dir().join("sync-body.md");
                fs::create_dir_all(store.dir()).ok();
                fs::write(&tmp, &text).context("write tmp body")?;
                let out = Proc::new("gh")
                    .args(["issue", "create", "--title", &title, "--body-file"])
                    .arg(&tmp)
                    .output()
                    .context("run `gh issue create`")?;
                if !out.status.success() {
                    anyhow::bail!(
                        "`gh issue create` failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
                // `gh issue create` prints the URL: https://github.com/OWNER/REPO/issues/<n>
                let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
                url.rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .with_context(|| format!("parse issue number from {url:?}"))?
            }
        }
    };

    // Save returned #n into frontmatter (create path, or offline demo).
    if fm.issue.is_none() {
        let updated = set_frontmatter_issue(&text, n)?;
        fs::write(&abs, updated).context("save issue number into frontmatter")?;
    }

    // Board refs: problem file (repo-relative if possible) + gh issue URL marker.
    let md_ref = abs
        .strip_prefix(&store.root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| abs.to_string_lossy().to_string());
    let gh_ref = format!("gh#{}", n);

    // Assert issue-opened for #N (stores gh issue #n as THE issue for this plan).
    let issue_id = format!("#{}", num);
    if !store.all_current()?.iter().any(|t| t.id == issue_id && t.r#type == "issue-opened") {
        let t = Tuple::new(
            &issue_id,
            "issue-opened",
            "done",
            "human",
            &format!("GitHub issue #{} opened/verified by deterministic sync.", n),
            vec![md_ref.clone(), gh_ref.clone()],
        );
        store.append(&t, true)?;
    }

    // Assert slice-open for the 5 slices (idempotent: skip ids already present).
    let existing_ids: std::collections::HashSet<String> =
        store.all_current()?.into_iter().map(|t| t.id).collect();
    for s in SYNC_SLICES {
        let id = format!("#{}{}", num, format!("/{}", s));
        if existing_ids.contains(&id) {
            continue;
        }
        let t = Tuple::new(
            &id,
            "slice-state",
            "open",
            "human",
            "",
            vec![md_ref.clone(), gh_ref.clone()],
        );
        store.append(&t, true)?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_null_then_set() {
        let doc = "---\nissue: null\n---\n\n# Hello\n";
        assert_eq!(parse_frontmatter(doc).unwrap().issue, None);
        let updated = set_frontmatter_issue(doc, 7).unwrap();
        assert!(updated.contains("issue: 7"));
        assert_eq!(parse_frontmatter(&updated).unwrap().issue, Some(7));
    }

    #[test]
    fn frontmatter_hash_number() {
        let doc = "---\nissue: 12 # filled by sync\n---\n\n# T\n";
        assert_eq!(parse_frontmatter(doc).unwrap().issue, Some(12));
    }
}
