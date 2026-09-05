//! #1/core + #1/board — local store: append-only JSONL log + rusqlite indexer.
//!
//! Reads hit the indexer only. No `git` subprocess anywhere in this crate.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::tuple::{current, Tuple};

pub const LOG_NAME: &str = "log.jsonl";
pub const DB_NAME: &str = "index.db";

#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf, // repo root containing `.blackboard/`
}

impl Store {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    pub fn dir(&self) -> PathBuf {
        self.root.join(".blackboard")
    }
    pub fn log_path(&self) -> PathBuf {
        self.dir().join(LOG_NAME)
    }
    pub fn db_path(&self) -> PathBuf {
        self.dir().join(DB_NAME)
    }

    /// `bb init` — idempotent.
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(self.dir()).context("create .blackboard")?;
        if !self.log_path().exists() {
            fs::write(self.log_path(), "").context("create log.jsonl")?;
        }
        let conn = self.open_db()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS current(
                 id TEXT PRIMARY KEY,
                 itype TEXT NOT NULL,
                 state TEXT NOT NULL,
                 actor TEXT NOT NULL,
                 summary TEXT NOT NULL,
                 ts TEXT NOT NULL,
                 refs TEXT NOT NULL
             );",
        )
        .context("create index table")?;
        Ok(())
    }

    fn open_db(&self) -> Result<Connection> {
        fs::create_dir_all(self.dir()).ok();
        Connection::open(self.db_path()).context("open index.db")
    }

    /// Append one tuple: validate, write JSONL, upsert indexer if newer.
    /// `allow_open=true` only for `bb sync`.
    pub fn append(&self, t: &Tuple, allow_open: bool) -> Result<()> {
        t.validate(allow_open).map_err(|e| anyhow::anyhow!(e))?;
        self.init()?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())
            .context("open log.jsonl")?;
        let line = serde_json::to_string(t).unwrap();
        writeln!(f, "{line}").context("append tuple")?;
        f.flush().ok();
        self.upsert_if_newer(t)
    }

    fn upsert_if_newer(&self, t: &Tuple) -> Result<()> {
        let conn = self.open_db()?;
        let existing: Option<Tuple> = conn
            .prepare("SELECT id,itype,state,actor,summary,ts,refs FROM current WHERE id=?1")
            .map(|mut st| {
                st.query_row([&t.id], |row| {
                    Ok(Tuple {
                        id: row.get(0)?,
                        r#type: row.get(1)?,
                        state: row.get(2)?,
                        actor: row.get(3)?,
                        summary: row.get(4)?,
                        ts: row.get(5)?,
                        refs: serde_json::from_str::<Vec<String>>(row.get::<_, String>(6)?.as_str())
                            .unwrap_or_default(),
                    })
                })
                .ok()
            })
            .unwrap_or(None);
        let newer = match existing {
            None => true,
            Some(ref e) => current([e, t]).map(|w| w.ts == t.ts && w.actor == t.actor).unwrap_or(true),
        };
        if newer {
            conn.execute(
                "INSERT INTO current(id,itype,state,actor,summary,ts,refs) VALUES(?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(id) DO UPDATE SET itype=excluded.itype,state=excluded.state,
                   actor=excluded.actor,summary=excluded.summary,ts=excluded.ts,refs=excluded.refs",
                params![
                    t.id,
                    t.r#type,
                    t.state,
                    t.actor,
                    t.summary,
                    t.ts,
                    serde_json::to_string(&t.refs).unwrap()
                ],
            )
            .context("upsert index")?;
        }
        Ok(())
    }

    /// Replay full JSONL log into the indexer (recovery path).
    #[allow(dead_code)]
    pub fn rebuild(&self) -> Result<usize> {
        self.init()?;
        let data = fs::read_to_string(self.log_path()).unwrap_or_default();
        let conn = self.open_db()?;
        conn.execute("DELETE FROM current", []).ok();
        drop(conn);
        let mut n = 0;
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let t: Tuple = serde_json::from_str(line).context("parse log line")?;
            self.upsert_if_newer(&t)?;
            n += 1;
        }
        Ok(n)
    }

    /// Read-only: all current tuples (indexer only, never git).
    pub fn all_current(&self) -> Result<Vec<Tuple>> {
        if !self.db_path().exists() {
            return Ok(vec![]);
        }
        let conn = Connection::open(self.db_path()).context("open index.db readonly")?;
        let mut st = conn.prepare("SELECT id,itype,state,actor,summary,ts,refs FROM current ORDER BY id")?;
        let rows = st.query_map([], |row| {
            Ok(Tuple {
                id: row.get(0)?,
                r#type: row.get(1)?,
                state: row.get(2)?,
                actor: row.get(3)?,
                summary: row.get(4)?,
                ts: row.get(5)?,
                refs: serde_json::from_str::<Vec<String>>(row.get::<_, String>(6)?.as_str())
                    .unwrap_or_default(),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn row_count(&self) -> usize {
        self.all_current().map(|v| v.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("bb-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Store::new(&dir)
    }

    #[test]
    fn init_append_rebuild_roundtrip() {
        let s = tmp_store("roundtrip");
        s.init().unwrap();
        let t = Tuple::new("#1/core", "slice-state", "planning", "agent-1", "starting", vec!["problems/x.md".into()]);
        s.append(&t, false).unwrap();
        assert_eq!(s.row_count(), 1);
        // delete db, rebuild from log
        fs::remove_file(s.db_path()).unwrap();
        let n = s.rebuild().unwrap();
        assert_eq!(n, 1);
        assert_eq!(s.row_count(), 1);
        let _ = fs::remove_dir_all(&s.root);
    }

    #[test]
    fn newer_wins() {
        let s = tmp_store("newer");
        let mut a = Tuple::new("#1/cli", "slice-state", "planning", "agent-1", "sketch", vec!["p".into()]);
        a.ts = "2026-09-05T10:00:00Z".into();
        let mut b = a.clone();
        b.state = "executing".into();
        b.summary = "building".into();
        b.ts = "2026-09-05T11:00:00Z".into();
        s.append(&a, false).unwrap();
        s.append(&b, false).unwrap();
        let all = s.all_current().unwrap();
        assert_eq!(all[0].state, "executing");
        let _ = fs::remove_dir_all(&s.root);
    }
}
