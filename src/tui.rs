//! #2/tabs — `ratatui` board with Open/Closed tabs over the same projection as `bb board`.
//!
//! Tab bar counts problems (issues), not slices. Problems render collapsed
//! (`▸ #N slug (k open|closed)`); the cursor-selected problem auto-expands
//! (`▾ …` + slice lines + refs). `↑/↓` (or `j/k`) moves the cursor.
//!
//! Three panes: tab bar, scrolling board, and a footer holding status, the
//! answer panel and the keybinding hint — the footer is sized to its content,
//! so it is never the thing that scrolls away.
use std::collections::BTreeMap;
use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs, Wrap};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::board::{collapsed_line, expanded_lines_animated, first_dispatchable, first_retryable, has_pending_question, log_tail_lines, partition_open_closed, pending_inline, projection, spinner_glyph, tab_counts, visible_problems, ProblemView, Tab, CLOSED_EMPTY, OPEN_EMPTY};
use crate::claude::Question;
use crate::dispatch::{read_sidecar, spawn_background_dispatch, submit_answers};
use crate::store::Store;

pub fn run_once(store: &Store, tab: Option<Tab>) -> Result<String> {
    use crate::board::render_board_filtered;
    Ok(render_board_filtered(store, tab)?.join("\n"))
}

/// In-TUI answer session: collects answers for every pending question of
/// one slice, then submits + auto-resumes without leaving the TUI.
struct AnswerSession {
    id: String,
    questions: Vec<Question>,
    answers: BTreeMap<String, String>,
    qi: usize,
    opt_cursor: usize,
    text_buf: String,
}

impl AnswerSession {
    fn start(id: String, questions: Vec<Question>) -> Self {
        AnswerSession { id, questions, answers: BTreeMap::new(), qi: 0, opt_cursor: 0, text_buf: String::new() }
    }

    fn current(&self) -> &Question {
        &self.questions[self.qi]
    }

    fn is_option_question(&self) -> bool {
        !self.current().options.is_empty()
    }

    /// Record the pending choice/text for the current question; returns
    /// true when every question is answered and the map is ready to submit.
    fn confirm_current(&mut self, answer: String) -> bool {
        let q = self.current().question.clone();
        self.answers.insert(q, answer);
        self.qi += 1;
        self.opt_cursor = 0;
        self.text_buf.clear();
        self.qi >= self.questions.len()
    }

    fn header(&self) -> String {
        format!("ANSWER {} ({}/{})", self.id, self.qi + 1, self.questions.len())
    }
}

/// Per-slice sidecar/log facts for the render path. Read once per indexer
/// refresh, not once per frame: `pending_inline`, `has_pending_question`
/// and `log_tail_lines` each hit the filesystem, and the draw closure runs
/// five times a second.
struct SliceExtras {
    question: Option<String>,
    has_question: bool,
    log_tail: Vec<String>,
}

/// Sidecar + log facts for every blocked slice, keyed by slice id
/// (`#3/dispatch`). Only blocked slices are read — the board asks about no
/// others.
fn read_extras(store: &Store, views: &[ProblemView]) -> BTreeMap<String, SliceExtras> {
    let mut out = BTreeMap::new();
    for p in views {
        for s in &p.slices {
            if s.state != "blocked" {
                continue;
            }
            let id = format!("#{}/{}", p.num, s.slice);
            let extras = SliceExtras {
                question: pending_inline(store, &id),
                has_question: has_pending_question(store, &id),
                log_tail: log_tail_lines(store, &id, 2),
            };
            out.insert(id, extras);
        }
    }
    out
}

/// Scroll offset that keeps the selected block (`sel_start..=sel_end`) on
/// screen in a `view_h`-row pane holding `total` lines. Clamps the end
/// first and the start second, so a problem taller than the pane shows its
/// head (`\u{25be} #N slug` + slices) rather than its tail.
fn follow_cursor(scroll: usize, sel_start: usize, sel_end: usize, total: usize, view_h: usize) -> usize {
    if view_h == 0 || total <= view_h {
        return 0;
    }
    let mut s = scroll;
    if sel_end >= s + view_h {
        s = sel_end + 1 - view_h;
    }
    if sel_start < s {
        s = sel_start;
    }
    s.min(total - view_h)
}

/// Rows one footer line occupies once wrapped to `width`. `Wrap { trim }`
/// drops leading blanks, so this is an upper bound — the footer is sized
/// generously rather than clipped.
fn wrapped_rows(s: &str, width: usize) -> usize {
    let n = s.chars().count();
    if n == 0 || width == 0 {
        1
    } else {
        n.div_ceil(width)
    }
}

/// True when a char keypress carries no modifier that changes its meaning.
/// SHIFT is allowed: crossterm reports an uppercase char as
/// `Char('A') + SHIFT`, so rejecting it drops every capital letter typed
/// into a free-text answer.
fn is_plain_char(k: &KeyEvent) -> bool {
    k.modifiers.difference(KeyModifiers::SHIFT).is_empty()
}

pub fn run_interactive(store: &Store, watch_ms: u64, initial: Option<Tab>) -> Result<()> {
    // Fresh screen every launch: alternate screen + clear on start,
    // restore on quit. Without this the old shell output stays behind
    // the board and quit leaves artifacts.
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;
    let watch = Duration::from_millis(watch_ms.clamp(200, 10_000));
    // Animation frames tick every 200ms so `executing` spinners visibly
    // move; indexer re-reads stay on the slower `watch` cadence. The tick
    // comes from elapsed time, not loop iterations: `event::poll` returns
    // early on input, so a counter would race the spinner while a key is held.
    let frame = Duration::from_millis(200);
    let started = std::time::Instant::now();
    let mut all = store.all_current().unwrap_or_default();
    let mut views = projection(&all);
    let mut extras = read_extras(store, &views);
    let mut last_refresh = std::time::Instant::now();
    // Set by any key that mutates the board (dispatch, answer) so the next
    // frame re-reads instead of showing up to `watch` ms of stale state.
    let mut dirty = false;
    let mut selected: usize = match initial {
        Some(Tab::Closed) => 1,
        _ => 0,
    };
    let mut cursor: usize = 0;
    // First board line on screen. Follows the cursor, so the selected
    // problem stays visible however long the board grows.
    let mut scroll: usize = 0;
    let mut status: Vec<String> = Vec::new();
    let mut answer: Option<AnswerSession> = None;
    let switch_tab = |selected: &mut usize, cursor: &mut usize, next: usize| {
        if *selected != next {
            *selected = next;
            *cursor = 0;
        }
    };
    let result = (|| -> Result<()> {
        loop {
            if dirty || last_refresh.elapsed() >= watch {
                all = store.all_current().unwrap_or_default();
                views = projection(&all);
                extras = read_extras(store, &views);
                last_refresh = std::time::Instant::now();
                dirty = false;
            }
            let tick = (started.elapsed().as_millis() / 200) as u64;
            let (n_open, n_closed) = tab_counts(&views);
            let cur = if selected == 0 { Tab::Open } else { Tab::Closed };
            let visible = visible_problems(&views, cur);
            if visible.is_empty() {
                cursor = 0;
            } else if cursor >= visible.len() {
                cursor = visible.len() - 1;
            }
            terminal.draw(|f| {
                let area = f.area();
                // Footer height is measured, not guessed: status lines and
                // the answer panel wrap, and a clipped footer hides the only
                // keybinding hint the UI has. Capped at half the screen so
                // the board never disappears behind a long message.
                let inner_w = area.width.saturating_sub(2).max(1) as usize;
                let help = if answer.is_some() {
                    "ANSWER MODE — ↑/↓ pick option (or 1-9) — type for free text — Enter confirm — Esc cancel".to_string()
                } else {
                    format!(
                        "↑/↓ select — Enter dispatch/retry agent — a answer waiting question — l show log — Tab/1/2/←/→ switch — q / Esc to quit — {} live, watch {}ms (indexer only, no git)",
                        spinner_glyph(tick),
                        watch_ms
                    )
                };
                let mut footer: Vec<(String, Style)> = Vec::new();
                for s in &status {
                    footer.push((s.clone(), Style::default().fg(Color::Green)));
                }
                if let Some(sess) = &answer {
                    for l in answer_panel(sess) {
                        footer.push((
                            l,
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ));
                    }
                }
                footer.push((help, Style::default()));
                let rows: usize = footer.iter().map(|(s, _)| wrapped_rows(s, inner_w)).sum();
                let footer_h = (rows + 2).clamp(3, (area.height / 2).max(3) as usize) as u16;
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Min(0),
                        Constraint::Length(footer_h),
                    ])
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
                let mut text: Vec<Line> = Vec::new();
                // Line span of the selected problem's block, so the scroll
                // offset below can keep all of it (or at least its head) on screen.
                let mut sel_start = 0usize;
                let mut sel_end = 0usize;
                if visible.is_empty() {
                    let msg = match cur {
                        Tab::Open => OPEN_EMPTY,
                        Tab::Closed => CLOSED_EMPTY,
                    };
                    text.push(Line::from(format!("  {msg}")));
                } else {
                    for (i, p) in visible.iter().enumerate() {
                        if i == cursor {
                            sel_start = text.len();
                            for (j, l) in expanded_lines_animated(p, cur, tick).iter().enumerate() {
                                if j == 0 {
                                    text.push(Line::from(Span::styled(
                                        l.clone(),
                                        Style::default()
                                            .fg(Color::Yellow)
                                            .add_modifier(Modifier::BOLD),
                                    )));
                                } else {
                                    text.push(Line::from(l.clone()));
                                }
                            }
                            // Pending-question inline: the turn is over, no agent
                            // process is alive — the question waits as data
                            // (sidecar) until answered, then `--resume`
                            // continues it. `?` carries the question text,
                            // WAITING names the action: press a, answer here.
                            for s in &p.slices {
                                if s.state != "blocked" {
                                    continue;
                                }
                                let id = format!("#{}/{}", p.num, s.slice);
                                let Some(ex) = extras.get(&id) else { continue };
                                if let Some(q) = &ex.question {
                                    text.push(Line::from(Span::styled(
                                        q.clone(),
                                        Style::default().fg(Color::Yellow),
                                    )));
                                }
                                if ex.has_question {
                                    text.push(Line::from(Span::styled(
                                        "  ? WAITING on you — press a to answer here (no live agent, the turn resumes after)",
                                        Style::default()
                                            .fg(Color::Yellow)
                                            .add_modifier(Modifier::BOLD),
                                    )));
                                }
                                for l in &ex.log_tail {
                                    text.push(Line::from(Span::styled(
                                        l.clone(),
                                        Style::default().fg(Color::Red),
                                    )));
                                }
                            }
                            sel_end = text.len().saturating_sub(1);
                        } else {
                            text.push(Line::from(collapsed_line(p, cur)));
                        }
                    }
                }
                // Scroll to keep the selection visible.
                let view_h = chunks[1].height.saturating_sub(2) as usize;
                let total = text.len();
                scroll = follow_cursor(scroll, sel_start, sel_end, total, view_h);
                let title = if total > view_h && view_h > 0 {
                    format!(" {}-{}/{} ", scroll + 1, (scroll + view_h).min(total), total)
                } else {
                    String::new()
                };
                // No wrap on the board: `.scroll()` counts pre-wrap lines,
                // so wrapping would desync the offset from the cursor (and
                // a long log tail would reflow the whole board). Long lines
                // clip at the right edge instead.
                let p = Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL).title(title))
                    .scroll((scroll as u16, 0));
                f.render_widget(p, chunks[1]);
                let ftext: Vec<Line> = footer
                    .into_iter()
                    .map(|(s, st)| Line::from(Span::styled(s, st)))
                    .collect();
                let fp = Paragraph::new(ftext)
                    .block(Block::default().borders(Borders::ALL))
                    .wrap(Wrap { trim: true });
                f.render_widget(fp, chunks[2]);
            })?;
            if event::poll(frame)? {
                if let Event::Key(k) = event::read()? {
                    // Windows reports press *and* release for every key;
                    // without this each keypress dispatches twice.
                    if k.kind != KeyEventKind::Press {
                        continue;
                    }
                    if answer.is_some() {
                        answer_key(k, &mut answer, &mut status, store);
                        dirty = true;
                        continue;
                    }
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Tab => {
                            let next = 1 - selected;
                            switch_tab(&mut selected, &mut cursor, next);
                        }
                        KeyCode::Char('1') => switch_tab(&mut selected, &mut cursor, 0),
                        KeyCode::Char('2') => switch_tab(&mut selected, &mut cursor, 1),
                        KeyCode::Left => switch_tab(&mut selected, &mut cursor, 0),
                        KeyCode::Right => switch_tab(&mut selected, &mut cursor, 1),
                        KeyCode::Up | KeyCode::Char('k') => {
                            cursor = cursor.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            cursor = cursor.saturating_add(1);
                        }
                        KeyCode::Enter => {
                            // Dispatch open slices; retry crashed-blocked
                            // slices (no pending question). Blocked-with-
                            // question is never re-dispatched — it needs
                            // `bb answer`, so Enter explains instead of
                            // orphaning the waiting session.
                            let all = store.all_current().unwrap_or_default();
                            let views = projection(&all);
                            let cur = if selected == 0 { Tab::Open } else { Tab::Closed };
                            let visible = visible_problems(&views, cur);
                            status = match visible.get(cursor) {
                                None => vec!["nothing to dispatch (empty tab)".to_string()],
                                Some(p) => {
                                    let (open, closed) = partition_open_closed(&p.slices);
                                    let picked: &[&crate::board::SliceView] = match cur {
                                        Tab::Open => &open,
                                        Tab::Closed => &closed,
                                    };
                                    let by = std::env::var("BB_ACTOR")
                                        .unwrap_or_else(|_| "tui-agent".to_string());
                                    let model = crate::dispatch::tui_model();
                                    if let Some(s) = first_dispatchable(picked) {
                                        let id = format!("#{}/{}", p.num, s.slice);
                                        match spawn_background_dispatch(store, &id, &by, None, &model, None) {
                                            Ok(log) => vec![format!(
                                                "dispatched {by} on {id} (model {model}, log {}) — watch for [!] + ! log tail; press l for full tail",
                                                log.display()
                                            )],
                                            Err(e) => vec![format!(
                                                "dispatch failed for {id}: {e:#}"
                                            )],
                                        }
                                    } else if let Some(s) = first_retryable(store, p.num, picked) {
                                        let id = format!("#{}/{}", p.num, s.slice);
                                        match spawn_background_dispatch(store, &id, &by, None, &model, None) {
                                            Ok(log) => vec![format!(
                                                "retrying crashed {id} as {by} (model {model}, log {}) — old [!] flips to [~] live",
                                                log.display()
                                            )],
                                            Err(e) => vec![format!(
                                                "retry failed for {id}: {e:#}"
                                            )],
                                        }
                                    } else {
                                        // No open + no retryable: either a
                                        // question is waiting, or nothing is actionable.
                                        let waiting = picked.iter().find(|s| {
                                            s.state == "blocked"
                                                && has_pending_question(store, &format!("#{}/{}", p.num, s.slice))
                                        });
                                        match waiting {
                                            Some(s) => vec![format!(
                                                "#{}/{} has a pending question — press a to answer here (or quit and bb answer '#{}/{}' --pick \"<label>\")",
                                                p.num, s.slice, p.num, s.slice
                                            )],
                                            None => vec![format!(
                                                "nothing dispatchable in #{} (no open or crashed-blocked slice)",
                                                p.num
                                            )],
                                        }
                                    }
                                }
                            };
                            dirty = true;
                        }
                        KeyCode::Char('a') => {
                            // Answer here: open an answer session for the
                            // selected problem's pending question (if any),
                            // then submit + auto-resume without leaving.
                            let all = store.all_current().unwrap_or_default();
                            let views = projection(&all);
                            let cur = if selected == 0 { Tab::Open } else { Tab::Closed };
                            let visible = visible_problems(&views, cur);
                            status = match visible.get(cursor) {
                                None => vec!["nothing to answer (empty tab)".to_string()],
                                Some(p) => {
                                    let target = p.slices.iter().find(|s| {
                                        s.state == "blocked"
                                            && has_pending_question(store, &format!("#{}/{}", p.num, s.slice))
                                    });
                                    match target {
                                        None => vec![format!(
                                            "no waiting question in #{} (nothing to answer)",
                                            p.num
                                        )],
                                        Some(s) => {
                                            let id = format!("#{}/{}", p.num, s.slice);
                                            match read_sidecar(store, &id) {
                                                Ok(Some(sc)) if !sc.questions.is_empty() && sc.answers.is_none() => {
                                                    answer = Some(AnswerSession::start(id.clone(), sc.questions.clone()));
                                                    vec![format!("answering {id} here — pick/type, Enter confirms")]
                                                }
                                                _ => vec![format!("no waiting question in {id} (already answered?)")],
                                            }
                                        }
                                    }
                                }
                            };
                        }
                        KeyCode::Char('l') => {
                            // Intuitive problem reader: show the dispatch-log
                            // tail for the selected problem's first blocked
                            // (else first) slice, plus the next action.
                            let all = store.all_current().unwrap_or_default();
                            let views = projection(&all);
                            let cur = if selected == 0 { Tab::Open } else { Tab::Closed };
                            let visible = visible_problems(&views, cur);
                            status = match visible.get(cursor) {
                                None => vec!["nothing selected (empty tab)".to_string()],
                                Some(p) => {
                                    let target = p.slices.iter()
                                        .find(|s| s.state == "blocked")
                                        .or_else(|| p.slices.first());
                                    match target {
                                        None => vec![format!("#{} has no slices yet", p.num)],
                                        Some(s) => {
                                            let id = format!("#{}/{}", p.num, s.slice);
                                            let mut out = Vec::new();
                                            match crate::dispatch::read_log_tail(store, &id, 5) {
                                                Ok(None) => out.push(format!("no dispatch log yet for {id} — press Enter to dispatch")),
                                                Ok(Some(tail)) if tail.is_empty() => out.push(format!("log empty for {id}")),
                                                Ok(Some(tail)) => {
                                                    out.push(format!("log for {id} (last {}):", tail.len()));
                                                    for l in tail {
                                                        let short: String = l.chars().take(160).collect();
                                                        out.push(format!("  {short}"));
                                                    }
                                                }
                                                Err(e) => out.push(format!("log read failed for {id}: {e:#}")),
                                            }
                                            // Next-action hint mirrors `bb log`.
                                            if let Ok(Some(sc)) = crate::dispatch::read_sidecar(store, &id) {
                                                if !sc.questions.is_empty() && sc.answers.is_none() {
                                                    out.push(format!("next: press a to answer '{id}' here (or quit and bb answer '{id}' --pick \"<label>\")"));
                                                } else {
                                                    out.push(format!("next: re-dispatch '{id}' (Enter) or bb log '{id}' outside TUI"));
                                                }
                                            } else {
                                                out.push(format!("next: re-dispatch '{id}' (Enter) or bb log '{id}' outside TUI"));
                                            }
                                            out
                                        }
                                    }
                                }
                            };
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    })();
    // Always restore: leave alternate screen (back to shell output),
    // show cursor, leave raw mode — even if the loop errored.
    crossterm::terminal::disable_raw_mode().ok();
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

/// Answer panel lines for the active session: header + current question +
/// numbered options (or the free-text buffer). Plain strings; the caller
/// styles them. Wrapping is handled by the board Paragraph.
fn answer_panel(sess: &AnswerSession) -> Vec<String> {
    let mut out = vec![sess.header(), format!("  {}", sess.current().question)];
    if sess.is_option_question() {
        for (i, o) in sess.current().options.iter().enumerate() {
            let mark = if i == sess.opt_cursor { ">" } else { " " };
            let desc = o.description.as_deref().unwrap_or("");
            out.push(format!("  {mark} {}. {} — {desc}", i + 1, o.label));
        }
    } else {
        out.push(format!("  > {}_", sess.text_buf));
    }
    out
}

/// One keypress while an answer session is active. Picking the last answer
/// submits (validate + sidecar + executing tick) and auto-spawns
/// `bb dispatch --resume` in the background, so the turn continues without
/// leaving the TUI. Esc cancels with nothing written.
fn answer_key(k: KeyEvent, answer: &mut Option<AnswerSession>, status: &mut Vec<String>, store: &Store) {
    let Some(mut sess) = answer.take() else { return };
    let by = std::env::var("BB_ACTOR").unwrap_or_else(|_| "tui-agent".to_string());
    let model = crate::dispatch::tui_model();
    let mut finished = false;
    match k.code {
        KeyCode::Esc => {
            *status = vec!["answer cancelled (nothing written)".to_string()];
            finished = true;
        }
        KeyCode::Up | KeyCode::Char('k') if sess.is_option_question() => {
            sess.opt_cursor = sess.opt_cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if sess.is_option_question() => {
            let n = sess.current().options.len();
            sess.opt_cursor = (sess.opt_cursor + 1).min(n.saturating_sub(1));
        }
        KeyCode::Enter if sess.is_option_question() => {
            let label = sess.current().options[sess.opt_cursor].label.clone();
            finished = submit_step(&mut sess, status, store, &by, &model, label);
        }
        KeyCode::Char(c) if sess.is_option_question() && is_plain_char(&k) && c.is_ascii_digit() => {
            let idx = (c as usize).saturating_sub('0' as usize + 1);
            if idx < sess.current().options.len() {
                let label = sess.current().options[idx].label.clone();
                finished = submit_step(&mut sess, status, store, &by, &model, label);
            }
        }
        KeyCode::Char(c) if !sess.is_option_question() && is_plain_char(&k) => {
            sess.text_buf.push(c);
        }
        KeyCode::Backspace if !sess.is_option_question() => {
            sess.text_buf.pop();
        }
        KeyCode::Enter => {
            let text = sess.text_buf.trim().to_string();
            if text.is_empty() {
                *status = vec!["type an answer first (or Esc to cancel)".to_string()];
            } else {
                finished = submit_step(&mut sess, status, store, &by, &model, text);
            }
        }
        _ => {}
    }
    if !finished {
        *answer = Some(sess);
    }
}

/// Record one answer; on the last question submit + auto-resume, on earlier
/// ones advance. Returns true when the session is over (submitted, resume
/// spawned or failed, cancelled by caller). On validation failure nothing
/// was written: the bad pick is dropped and the session stays open on the
/// same question (returns false).
fn submit_step(
    sess: &mut AnswerSession,
    status: &mut Vec<String>,
    store: &Store,
    by: &str,
    model: &str,
    picked: String,
) -> bool {
    if !sess.confirm_current(picked) {
        return false;
    }
    let id = sess.id.clone();
    let detail = sess.answers.iter().map(|(q, a)| format!("{q}={a}")).collect::<Vec<_>>().join(", ");
    match submit_answers(store, &id, by, sess.answers.clone()) {
        Ok(sid) => {
            match spawn_background_dispatch(store, &id, by, None, model, Some(&sid)) {
                Ok(log) => {
                    *status = vec![format!(
                        "answered {id} ({detail}) — resuming {sid} (model {model}, log {})",
                        log.display()
                    )];
                }
                Err(e) => {
                    *status = vec![format!("answered {id} ({detail}) but resume failed: {e:#}")];
                }
            }
            true
        }
        Err(e) => {
            sess.qi = sess.qi.saturating_sub(1);
            let q = sess.current().question.clone();
            sess.answers.remove(&q);
            *status = vec![format!("answer rejected: {e:#}")];
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::QOption;

    fn option_q(text: &str, labels: &[&str]) -> Question {
        Question {
            question: text.into(),
            header: None,
            options: labels.iter().map(|l| QOption { label: l.to_string(), description: None }).collect(),
            multi_select: Some(false),
        }
    }

    fn text_q(text: &str) -> Question {
        Question { question: text.into(), header: None, options: vec![], multi_select: None }
    }

    #[test]
    fn follow_cursor_keeps_selection_on_screen() {
        // Fits: never scrolls.
        assert_eq!(follow_cursor(0, 0, 3, 10, 20), 0);
        // Selection below the fold: scrolls just far enough to show its end.
        assert_eq!(follow_cursor(0, 40, 42, 60, 10), 33);
        // Selection above the current offset: scrolls back to its start.
        assert_eq!(follow_cursor(30, 5, 7, 60, 10), 5);
        // Block taller than the pane: start wins, head stays visible.
        assert_eq!(follow_cursor(0, 20, 45, 60, 10), 20);
        // Never scrolls past the last screenful.
        assert_eq!(follow_cursor(99, 0, 0, 60, 10), 0);
        assert_eq!(follow_cursor(0, 59, 59, 60, 10), 50);
        // Degenerate pane height.
        assert_eq!(follow_cursor(7, 3, 4, 60, 0), 0);
    }

    #[test]
    fn wrapped_rows_counts_footer_height() {
        assert_eq!(wrapped_rows("", 20), 1);
        assert_eq!(wrapped_rows("abc", 20), 1);
        assert_eq!(wrapped_rows(&"x".repeat(20), 20), 1);
        assert_eq!(wrapped_rows(&"x".repeat(21), 20), 2);
        assert_eq!(wrapped_rows("abc", 0), 1);
    }

    #[test]
    fn shifted_chars_still_type() {
        // crossterm reports an uppercase char as Char('A') + SHIFT; dropping
        // it would silently swallow every capital in a free-text answer.
        assert!(is_plain_char(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)));
        assert!(is_plain_char(&KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)));
        assert!(!is_plain_char(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(!is_plain_char(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT)));
    }

    #[test]
    fn answer_session_advances_one_question_at_a_time() {
        let mut s = AnswerSession::start("#9/probe".into(), vec![option_q("color?", &["red", "blue"]), text_q("name?")]);
        assert_eq!(s.header(), "ANSWER #9/probe (1/2)");
        assert!(s.is_option_question());
        assert!(!s.confirm_current("red".into()));
        assert_eq!(s.header(), "ANSWER #9/probe (2/2)");
        assert!(!s.is_option_question());
        assert!(s.confirm_current("bob".into()));
        assert_eq!(s.answers["color?"], "red");
        assert_eq!(s.answers["name?"], "bob");
    }

    #[test]
    fn answer_panel_shows_options_with_cursor() {
        let s = AnswerSession::start("#9/probe".into(), vec![option_q("color?", &["red", "blue"])]);
        let panel = answer_panel(&s);
        assert!(panel[0].contains("#9/probe (1/1)"), "{panel:?}");
        assert!(panel.iter().any(|l| l.contains("> 1. red")), "{panel:?}");
        assert!(panel.iter().any(|l| l.contains("2. blue")), "{panel:?}");
    }

    #[test]
    fn submit_step_rejects_bad_label_without_spawning() {
        let dir = std::env::temp_dir().join(format!("bb-tui-answer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        store.init().unwrap();
        let sc = crate::dispatch::Sidecar {
            session_id: "sess-tui".into(),
            tool_use_id: "tu".into(),
            questions: vec![option_q("color?", &["red", "blue"])],
            answers: None,
        };
        crate::dispatch::write_sidecar(&store, "#9/probe", &sc).unwrap();
        let mut sess = AnswerSession::start("#9/probe".into(), vec![option_q("color?", &["red", "blue"])]);
        let mut status = Vec::new();
        // Invalid label: validation fails before write/spawn, session stays
        // open on the same question.
        assert!(!submit_step(&mut sess, &mut status, &store, "tui", "sonnet", "green".into()));
        assert_eq!(sess.qi, 0);
        assert!(sess.answers.is_empty());
        assert!(status[0].contains("rejected"));
        let back = crate::dispatch::read_sidecar(&store, "#9/probe").unwrap().unwrap();
        assert!(back.answers.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
