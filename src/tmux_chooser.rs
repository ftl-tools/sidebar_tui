//! Standalone chooser: native tmux renders workloads; this screen only renders metadata.
use crate::{
    colors,
    hint_bar::{HintBar, KeybindingInfo},
    tmux::{Adapter, PaneTarget, Snapshot, Socket, missing_server},
};
use anyhow::{Context, Result, ensure};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Block, Borders, ListState, Paragraph},
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Row {
    target: PaneTarget,
    label: String,
}
fn rows(snapshot: &Snapshot) -> Vec<Row> {
    let mut rows = vec![];
    for m in &snapshot.memberships {
        let s = snapshot
            .sessions
            .iter()
            .find(|s| s.id == m.session)
            .expect("validated session");
        let w = snapshot
            .windows
            .iter()
            .find(|w| w.id == m.window)
            .expect("validated window");
        for p in snapshot.panes.iter().filter(|p| p.window == m.window) {
            rows.push(Row {
                target: PaneTarget {
                    server: snapshot.server.clone(),
                    session: m.session.clone(),
                    window: m.window.clone(),
                    pane: p.id.clone(),
                },
                label: format!(
                    // Trailing indicators were clipped by long names; IDs/focus come first.
                    "{}/{}/{} [{}{}] {:?} / {:?} / {:?}",
                    s.id.as_str(),
                    w.id.as_str(),
                    p.id.as_str(),
                    if m.active { "W*" } else { "W-" },
                    if p.active { " P*" } else { " P-" },
                    s.name,
                    w.name,
                    p.title
                ),
            });
        }
    }
    rows
}
fn same_target(a: &PaneTarget, b: &PaneTarget) -> bool {
    a.server.same_generation(&b.server)
        && a.session == b.session
        && a.window == b.window
        && a.pane == b.pane
}
#[derive(Default)]
struct Model {
    rows: Vec<Row>,
    selected: Option<PaneTarget>,
    query: String,
    message: String,
    empty: bool,
}
impl Model {
    fn visible(&self) -> Vec<&Row> {
        let q = self.query.to_lowercase();
        self.rows
            .iter()
            .filter(|r| r.label.to_lowercase().contains(&q))
            .collect()
    }
    fn refresh(&mut self, result: Result<Snapshot>, initial: bool) {
        match result {
            Ok(s) => {
                self.empty = s.sessions.is_empty();
                self.rows = rows(&s);
                if initial {
                    self.selected = self.rows.first().map(|r| r.target.clone());
                }
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|t| !self.rows.iter().any(|r| same_target(t, &r.target)))
                {
                    self.selected = None;
                    self.message =
                        "Selected target disappeared or server restarted; navigate to choose again"
                            .into();
                }
            }
            Err(e) => {
                self.empty = missing_server(&e);
                self.rows.clear();
                self.selected = None;
                self.message = format!("Disconnected: {e:#}");
            }
        }
    }
    fn navigate(&mut self, down: bool) {
        let visible = self.visible();
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|s| visible.iter().position(|r| same_target(s, &r.target)));
        let index = match current {
            Some(i) if down => (i + 1).min(visible.len() - 1),
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.selected = Some(visible[index].target.clone());
    }
    fn search_changed(&mut self) {
        self.selected = self.visible().first().map(|r| r.target.clone());
    }
}
struct Screen;
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}
fn screen() -> Result<(Screen, Terminal<CrosstermBackend<io::Stdout>>)> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "The tmux chooser requires a terminal; use list-sessions --json for scripts"
    );
    enable_raw_mode()?;
    let guard = Screen;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    Ok((guard, terminal))
}

pub fn run(adapter: Adapter, cwd: Option<PathBuf>, requested_client: Option<String>) -> Result<()> {
    adapter.version()?;
    let context = std::env::var("TMUX").ok().filter(|c| !c.is_empty());
    ensure!(
        context.is_some() || requested_client.is_none(),
        "--client is only valid inside tmux"
    );
    let origin = if let Some(c) = context {
        let origin_adapter = Adapter::new(Socket::resolve(None, None, Some(c))?);
        ensure!(
            origin_adapter.identity()? == adapter.identity()?,
            "Cannot switch across tmux servers from inside tmux; launch outside instead"
        );
        Some(
            std::env::var("TMUX_PANE")
                .context("TMUX_PANE missing; cannot identify the invoking client safely")?,
        )
    } else {
        None
    };
    // The invoking pane/client is also generation-scoped; a restarted server cannot revive it.
    let origin_identity = if origin.is_some() {
        Some(adapter.identity()?)
    } else {
        None
    };
    if let Some(ref cwd) = cwd {
        ensure!(
            cwd.is_absolute() && cwd.is_dir(),
            "--cwd must be an existing absolute directory"
        );
    }
    let mut model = Model::default();
    model.refresh(adapter.snapshot(), true);
    let (guard, mut terminal) = screen()?;
    let mut last = Instant::now();
    let mut search = false;
    let mut confirm_create = false;
    let mut list_state = ListState::default();
    let chosen = loop {
        let visible = model.visible();
        list_state.select(
            model
                .selected
                .as_ref()
                .and_then(|s| visible.iter().position(|r| same_target(s, &r.target))),
        );
        terminal.draw(|f| {
            let regions = Layout::vertical([Constraint::Length(2), Constraint::Min(1), Constraint::Length(7)]).split(f.area());
            f.render_widget(Paragraph::new(format!("Sidebar tmux chooser — browse only; Enter commits\nSearch{}: {}", if search { " (typing)" } else { "" }, model.query)), regions[0]);
            let labels: Vec<_> = visible.iter().map(|r| r.label.clone()).collect();
            crate::sidebar::render_chooser_list(f, regions[1], &labels, &mut list_state);
            let message = if confirm_create {
                format!("Create a native session in {:?}? y confirms; n/Esc cancels. tmux uses its normal config/environment.", cwd.as_ref().unwrap())
            } else if model.empty { "No sessions. c creates only with explicit --cwd; q leaves everything unchanged".into() }
            else { model.message.clone() };
            // Normal hints were misleading in prompts; keep confirmation/search controls visible.
            let hints = if confirm_create {
                HintBar::new(vec![KeybindingInfo::new("y", "Create native session"), KeybindingInfo::new("n/Esc", "Cancel creation")], "Ctrl+C Exit chooser")
            } else if search {
                HintBar::new(vec![KeybindingInfo::new("Enter", "Finish search (no attach)"), KeybindingInfo::new("Esc", "Clear search"), KeybindingInfo::new("Backspace", "Edit query")], "Ctrl+C Exit chooser")
            } else {
                HintBar::new(vec![KeybindingInfo::new("j/k arrows", "Browse"), KeybindingInfo::new("Enter", "Commit native target"), KeybindingInfo::new("/", "Search"), KeybindingInfo::new("r", "Refresh")], "q/Esc Cancel")
            };
            let bottom = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(regions[2]);
            f.render_widget(Paragraph::new(message).block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(colors::PURPLE))), bottom[0]);
            f.render_widget(hints, bottom[1]);
        })?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break None;
                }
                if confirm_create {
                    match key.code {
                        KeyCode::Char('y') => {
                            confirm_create = false;
                            model.message = match adapter.create_empty(cwd.as_ref().unwrap()) {
                                Ok(()) => "Created. Navigate and Enter to attach.".into(),
                                Err(e) => format!("{e:#}"),
                            };
                            model.refresh(adapter.snapshot(), false);
                        }
                        KeyCode::Esc | KeyCode::Char('n') => confirm_create = false,
                        _ => {}
                    }
                } else if search {
                    match key.code {
                        KeyCode::Esc => {
                            model.query.clear();
                            search = false;
                        }
                        KeyCode::Enter => search = false,
                        KeyCode::Backspace => {
                            model.query.pop();
                            model.search_changed();
                        }
                        KeyCode::Char(c)
                            if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                        {
                            model.query.push(c);
                            model.search_changed();
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => break None,
                        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('n') => {
                            model.navigate(true)
                        }
                        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('p') => {
                            model.navigate(false)
                        }
                        KeyCode::Char('/') => search = true,
                        KeyCode::Char('r') => {
                            model.message.clear();
                            model.refresh(adapter.snapshot(), false);
                            last = Instant::now();
                        }
                        KeyCode::Char('c') if model.empty => {
                            if cwd.is_some() {
                                confirm_create = true;
                            } else {
                                model.message =
                                    "Reopen with an existing absolute --cwd to enable creation"
                                        .into();
                                model.empty = false;
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(target) = model.selected.clone().filter(|t| {
                                model.visible().iter().any(|r| same_target(t, &r.target))
                            }) {
                                // Resolve before leaving the UI; stale targets never fall through to a new row.
                                match adapter.resolve_target(&target) {
                                    Ok(fresh) => {
                                        let client = if let Some(ref pane) = origin {
                                            if adapter.identity().ok().as_ref()
                                                != origin_identity.as_ref()
                                            {
                                                model.message = "Invoking server restarted; exit and reopen chooser".into();
                                                continue;
                                            }
                                            match adapter.choose_client(
                                                &fresh,
                                                pane,
                                                requested_client.as_deref(),
                                            ) {
                                                Ok(c) => Some(c),
                                                Err(e) => {
                                                    model.message = format!("{e:#}");
                                                    continue;
                                                }
                                            }
                                        } else {
                                            None
                                        };
                                        break Some((target, client));
                                    }
                                    Err(e) => {
                                        model.message = format!("{e:#}");
                                        model.selected = None;
                                    }
                                }
                            }
                        }
                        _ => {} // Never forward chooser keys to a workload.
                    }
                }
            }
        }
        if last.elapsed() >= Duration::from_millis(750) {
            model.refresh(adapter.snapshot(), false);
            last = Instant::now();
        }
    };
    // The legacy UI renders terminals itself; native attachment must happen only after restoration.
    drop(terminal);
    drop(guard);
    if let Some((target, client)) = chosen {
        if let Some(ref pane) = origin {
            ensure!(
                Some(adapter.identity()?) == origin_identity,
                "Invoking server restarted; reopen chooser"
            );
            let fresh = adapter.resolve_target(&target)?;
            ensure!(
                adapter.choose_client(&fresh, pane, requested_client.as_deref())?
                    == *client.as_ref().unwrap(),
                "Invoking client changed; reopen chooser"
            );
        }
        adapter.commit(&target, client.as_deref())?;
        if client.is_none() {
            let status = adapter
                .command()
                .args(["attach-session", "-t", target.session.as_str()])
                .status()?;
            ensure!(
                status.success(),
                "Native tmux attach failed; terminal restored, no action replayed"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::{PaneId, ServerIdentity, SessionId, WindowId};
    fn row(id: &str) -> Row {
        Row {
            target: PaneTarget {
                server: ServerIdentity {
                    socket: "s".into(),
                    pid: 1,
                    started: 1,
                    observation: "x".into(),
                },
                session: SessionId::parse("$0").unwrap(),
                window: WindowId::parse("@0").unwrap(),
                pane: PaneId::parse(id).unwrap(),
            },
            label: id.into(),
        }
    }
    #[test]
    fn browsing_and_search_do_not_commit() {
        let mut m = Model {
            rows: vec![row("%0"), row("%1")],
            ..Default::default()
        };
        m.navigate(true);
        assert_eq!(m.selected.as_ref().unwrap().pane.as_str(), "%0");
        m.navigate(true);
        assert_eq!(m.selected.as_ref().unwrap().pane.as_str(), "%1");
        m.query = "%0".into();
        m.search_changed();
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.selected.unwrap().pane.as_str(), "%0");
    }
    #[test]
    fn generation_and_membership_are_part_of_selection() {
        let a = row("%0");
        let mut b = a.clone();
        b.target.server.observation = "another read".into();
        assert!(same_target(&a.target, &b.target));
        b.target.session = SessionId::parse("$1").unwrap();
        assert!(!same_target(&a.target, &b.target));
        b = a.clone();
        b.target.server.pid += 1;
        assert!(!same_target(&a.target, &b.target));
        let mut m = Model {
            rows: vec![a.clone()],
            selected: Some(a.target),
            ..Default::default()
        };
        m.refresh(Err(anyhow::anyhow!("offline")), false);
        assert!(m.rows.is_empty() && m.selected.is_none() && !m.empty);
    }
}
