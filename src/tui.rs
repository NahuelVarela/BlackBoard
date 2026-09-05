//! #1/tui — `ratatui` board over the same projection as `bb board`, with watch mode.
use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::board::render_board;
use crate::store::Store;

pub fn run_once(store: &Store) -> Result<String> {
    Ok(render_board(store)?.join("\n"))
}

pub fn run_interactive(store: &Store, watch_ms: u64) -> Result<()> {
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    let poll = Duration::from_millis(watch_ms.clamp(200, 10_000));
    let result = (|| -> Result<()> {
        loop {
            let lines = render_board(store)?;
            terminal.draw(|f| {
                let area = f.area();
                let text = format!("{}\n\nq / Esc to quit — watch {}ms (indexer only, no git)", lines.join("\n"), watch_ms);
                let p = Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL).title(" Blackboard "));
                f.render_widget(p, area);
            })?;
            if event::poll(poll)? {
                if let Event::Key(k) = event::read()? {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        _ => {}
                    }
                }
            }
            // re-read indexer each tick (watch mode); store handle is cheap to reuse
            let _ = store.row_count();
        }
        Ok(())
    })();
    crossterm::terminal::disable_raw_mode()?;
    result
}
