//! #2/tabs — `ratatui` board with Open/Closed tabs over the same projection as `bb board`.
use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::board::{projection, render_board_filtered, tab_counts, Tab};
use crate::store::Store;

pub fn run_once(store: &Store, tab: Option<Tab>) -> Result<String> {
    Ok(render_board_filtered(store, tab)?.join("\n"))
}

pub fn run_interactive(store: &Store, watch_ms: u64, initial: Option<Tab>) -> Result<()> {
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    let poll = Duration::from_millis(watch_ms.clamp(200, 10_000));
    let mut selected: usize = match initial {
        Some(Tab::Closed) => 1,
        _ => 0,
    };
    let result = (|| -> Result<()> {
        loop {
            let all = store.all_current().unwrap_or_default();
            let views = projection(&all);
            let (n_open, n_closed) = tab_counts(&views);
            let cur = if selected == 0 { Tab::Open } else { Tab::Closed };
            let lines = render_board_filtered(store, Some(cur)).unwrap_or_default();
            terminal.draw(|f| {
                let area = f.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(0)])
                    .split(area);
                let titles = vec![
                    format!("Open ({n_open})"),
                    format!("Closed ({n_closed})"),
                ];
                let tabs = Tabs::new(titles)
                    .block(Block::default().borders(Borders::ALL).title(" Blackboard "))
                    .select(selected)
                    .style(Style::default().fg(Color::White))
                    .highlight_style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_widget(tabs, chunks[0]);
                let text = format!(
                    "{}\n\nTab/1/2/←/→ switch — q / Esc to quit — watch {}ms (indexer only, no git)",
                    lines.join("\n"),
                    watch_ms
                );
                let p = Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL));
                f.render_widget(p, chunks[1]);
            })?;
            if event::poll(poll)? {
                if let Event::Key(k) = event::read()? {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Tab => selected = 1 - selected,
                        KeyCode::Char('1') => selected = 0,
                        KeyCode::Char('2') => selected = 1,
                        KeyCode::Left => selected = 0,
                        KeyCode::Right => selected = 1,
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
