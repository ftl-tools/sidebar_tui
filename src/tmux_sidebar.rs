//! Step 3 architecture prototype: only explicitly created private demo servers are eligible.
#[cfg(unix)]
mod native {
    use crate::{
        hint_bar::{HintBar, KeybindingInfo},
        tmux::{Adapter, PaneId, ServerIdentity, Snapshot, WindowId},
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
        widgets::{ListState, Paragraph},
    };
    use std::{
        fs::{File, OpenOptions},
        io,
        os::{
            fd::AsRawFd,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
        path::Path,
        time::{Duration, Instant},
    };

    const OWNER: &str = "sidebar-step3-v1";
    const OWNER_KEY: &str = "@sb_demo_owner";
    const SESSION_KEY: &str = "@sb_demo_session";
    const WORK_KEY: &str = "@sb_demo_work";
    const PANEL_KEY: &str = "@sb_demo_panel";
    const BINDING_KEY: &str = "@sb_demo_binding";

    // Kernel locks survive concurrent launch attempts but not a crashed owner. Never unlink
    // the lock file: unlinking permits two processes to lock different inodes at the same path.
    struct Lock(File);
    impl Drop for Lock {
        fn drop(&mut self) {
            unsafe {
                libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
    fn lock(a: &Adapter) -> Result<Lock> {
        let path = Path::new(a.explicit_socket_path()?);
        ensure!(path.is_absolute(), "Demo socket must be absolute");
        let parent = path.parent().context("Socket parent missing")?;
        let md = parent.metadata()?;
        ensure!(
            md.is_dir() && md.uid() == unsafe { libc::geteuid() } && md.mode() & 0o077 == 0,
            "Use an owned private directory (mode 0700) for the disposable demo socket"
        );
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(parent.join(".sidebar_demo.lock"))?;
        ensure!(
            file.metadata()?.is_file() && file.metadata()?.uid() == unsafe { libc::geteuid() },
            "Invalid demo lock ownership"
        );
        let started = Instant::now();
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Lock(file));
            }
            ensure!(
                started.elapsed() < Duration::from_secs(5),
                "Another demo operation is busy; retry later"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn request(a: &Adapter, args: &[&str]) -> Result<String> {
        let out = a.command().args(args).output()?;
        ensure!(
            out.status.success(),
            "tmux demo operation failed (not replayed): {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(String::from_utf8(out.stdout)?.trim_end_matches('\n').into())
    }
    fn global(a: &Adapter, key: &str) -> Result<String> {
        request(a, &["show-options", "-gqv", key])
    }
    fn window_option(a: &Adapter, window: &str, key: &str) -> Result<String> {
        request(a, &["show-options", "-wqv", "-t", window, key])
    }
    fn marked(a: &Adapter, pane: &str) -> Result<bool> {
        Ok(request(a, &["show-options", "-pqv", "-t", pane, PANEL_KEY])? == OWNER)
    }
    #[derive(Clone)]
    struct Entry {
        window: WindowId,
        name: String,
        work: PaneId,
        panel: Option<PaneId>,
    }
    struct Inventory {
        snapshot: Snapshot,
        session: String,
        entries: Vec<Entry>,
    }
    fn inventory(a: &Adapter) -> Result<Inventory> {
        ensure!(
            global(a, OWNER_KEY)? == OWNER,
            "Refusing an unowned server; create a fresh sidebar-demo on a private socket"
        );
        let snapshot = a.snapshot()?;
        let session = global(a, SESSION_KEY)?;
        ensure!(
            snapshot.sessions.len() == 1 && snapshot.sessions[0].id.as_str() == session,
            "Demo topology changed: expected only the owned session; use the chooser instead"
        );
        let mut entries = vec![];
        for m in &snapshot.memberships {
            let work = window_option(a, m.window.as_str(), WORK_KEY)?;
            // Native-created or linked windows are not enrolled implicitly.
            if work.is_empty() {
                continue;
            }
            ensure!(
                m.session.as_str() == session
                    && snapshot
                        .memberships
                        .iter()
                        .filter(|n| n.window == m.window)
                        .count()
                        == 1,
                "Linked windows are outside the Step 3 demo contract"
            );
            let work = PaneId::parse(&work)?;
            let panes: Vec<_> = snapshot
                .panes
                .iter()
                .filter(|p| p.window == m.window)
                .collect();
            ensure!(
                panes.iter().any(|p| p.id == work),
                "Working pane disappeared; no automatic replacement"
            );
            let mut panel = None;
            for p in panes {
                if p.id == work {
                    ensure!(
                        !marked(a, p.id.as_str())?,
                        "Working pane cannot be marked as Sidebar"
                    );
                    continue;
                }
                ensure!(
                    marked(a, p.id.as_str())?,
                    "Extra working split detected; use the chooser until pane management is supported"
                );
                // A user can respawn a marked pane with another application. The role marker
                // alone then becomes stale; never close that replacement workload as Sidebar.
                ensure!(
                    request(
                        a,
                        &["show-options", "-pqv", "-t", p.id.as_str(), "@sb_demo_pid"]
                    )? == p.pid.to_string(),
                    "Marked Sidebar process was replaced externally; leaving the pane untouched"
                );
                ensure!(
                    panel.is_none(),
                    "Duplicate marked sidebars detected; refusing cleanup by guessing"
                );
                panel = Some(p.id.clone());
            }
            let name = snapshot
                .windows
                .iter()
                .find(|w| w.id == m.window)
                .context("Window disappeared")?
                .name
                .clone();
            entries.push(Entry {
                window: m.window.clone(),
                name,
                work,
                panel,
            });
        }
        ensure!(!entries.is_empty(), "No marked demo windows remain");
        Ok(Inventory {
            snapshot,
            session,
            entries,
        })
    }
    fn generation(a: &Adapter, expected: &ServerIdentity) -> Result<()> {
        let now = a.snapshot()?;
        ensure!(
            now.server.same_generation(expected),
            "Demo server restarted; discard old targets and reopen"
        );
        Ok(())
    }
    fn enroll(a: &Adapter) -> Result<Inventory> {
        let before = inventory(a)?;
        for e in &before.entries {
            if e.panel.is_some() {
                continue;
            }
            generation(a, &before.snapshot.server)?;
            let exe = std::env::current_exe()?;
            // tmux executes multiple shell-command arguments directly; no shell string or name interpolation.
            let out = a
                .command()
                .args([
                    "split-window",
                    "-d",
                    "-h",
                    "-b",
                    "-l",
                    "28",
                    "-t",
                    e.work.as_str(),
                    "-P",
                    "-F",
                    "#{pane_id} #{pane_pid}",
                ])
                .arg(exe)
                .args([
                    "tmux",
                    "--socket-path",
                    a.explicit_socket_path()?,
                    "sidebar-pane",
                ])
                .output()?;
            ensure!(
                out.status.success(),
                "Cannot insert demo sidebar; existing workloads retained: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let identity = String::from_utf8(out.stdout)?;
            let fields: Vec<_> = identity.split_whitespace().collect();
            ensure!(
                fields.len() == 2 && fields[1].parse::<u64>().is_ok(),
                "Missing new Sidebar process identity"
            );
            let id = PaneId::parse(fields[0])?;
            request(
                a,
                &[
                    "set-option",
                    "-p",
                    "-t",
                    id.as_str(),
                    "@sb_demo_pid",
                    fields[1],
                ],
            )?;
            request(
                a,
                &["set-option", "-p", "-t", id.as_str(), PANEL_KEY, OWNER],
            )?;
        }
        inventory(a)
    }
    fn current_binding(a: &Adapter) -> Result<String> {
        // Table-column parsing missed bindings with flags such as -r. Ask for the exact
        // key and retain tmux's canonical command serialization for reversible ownership.
        let out = a
            .command()
            .args(["list-keys", "-T", "root", "F12"])
            .output()?;
        if out.status.success() {
            return Ok(String::from_utf8(out.stdout)?.trim_end_matches('\n').into());
        }
        ensure!(
            String::from_utf8_lossy(&out.stderr).trim() == "unknown key: F12",
            "Cannot inspect F12 binding: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(String::new())
    }
    // Native if-shell branches select a literal pane ID before subsequent keys are processed.
    // A background run-shell launcher would leave a keystroke-leak window while the shell is focused.
    fn binding_command(entries: &[Entry]) -> Result<String> {
        let mut command = "display-message 'Sidebar unavailable; run sidebar-show'".to_string();
        for e in entries.iter().rev() {
            let pane = e.panel.as_ref().context("Sidebar missing for binding")?;
            command = format!(
                "if-shell -F '#{{==:#{{window_id}},{}}}' 'select-pane -t {}' {}",
                e.window.as_str(),
                pane.as_str(),
                quote(&command)
            );
        }
        Ok(command)
    }
    fn quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
    fn binding(a: &Adapter, entries: &[Entry], enable: bool) -> Result<()> {
        let owned = global(a, BINDING_KEY)?;
        if !enable && owned.is_empty() {
            return Ok(());
        }
        let current = current_binding(a)?;
        ensure!(
            current == owned,
            "F12 binding collision or later user edit; not overwriting it"
        );
        let command = binding_command(entries)?;
        request(a, &["bind-key", "-T", "root", "F12", &command])?;
        let installed = current_binding(a)?;
        ensure!(
            !installed.is_empty(),
            "Installed binding could not be identified"
        );
        request(a, &["set-option", "-g", BINDING_KEY, &installed])?;
        Ok(())
    }
    pub fn demo(a: &Adapter, cwd: &Path, enable_binding: bool) -> Result<()> {
        let _lock = lock(a)?;
        a.version()?;
        ensure!(
            cwd.is_absolute() && cwd.is_dir(),
            "Demo needs an existing absolute --cwd"
        );
        match a.snapshot() {
            Ok(_) => {
                inventory(a)?;
            }
            Err(e) => {
                ensure!(crate::tmux::missing_server(&e), "Cannot create demo: {e:#}");
                let cwd = cwd
                    .to_str()
                    .context("cwd must be UTF-8")?
                    .replace('#', "##");
                // A fresh dedicated server uses no user config. Never start/alter the default server.
                let out = a
                    .command_with_start(true)
                    .args([
                        "-f",
                        "/dev/null",
                        "new-session",
                        "-d",
                        "-s",
                        "sidebar-demo",
                        "-n",
                        "editor",
                        "-c",
                        &cwd,
                        ";",
                        "set-option",
                        "-g",
                        OWNER_KEY,
                        OWNER,
                        ";",
                        "set-option",
                        "-g",
                        "exit-empty",
                        "off",
                    ])
                    .output()?;
                ensure!(
                    out.status.success(),
                    "Demo creation failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                let first = a.snapshot()?;
                let sid = first.sessions[0].id.as_str();
                request(a, &["set-option", "-g", SESSION_KEY, sid])?;
                request(
                    a,
                    &[
                        "set-option",
                        "-w",
                        "-t",
                        first.windows[0].id.as_str(),
                        WORK_KEY,
                        first.panes[0].id.as_str(),
                    ],
                )?;
                let second = request(
                    a,
                    &[
                        "new-window",
                        "-d",
                        "-t",
                        sid,
                        "-n",
                        "logs",
                        "-c",
                        &cwd,
                        "-P",
                        "-F",
                        "#{window_id} #{pane_id}",
                    ],
                )?;
                let parts: Vec<_> = second.split_whitespace().collect();
                ensure!(parts.len() == 2, "Missing newly created window identity");
                WindowId::parse(parts[0])?;
                PaneId::parse(parts[1])?;
                request(a, &["set-option", "-w", "-t", parts[0], WORK_KEY, parts[1]])?;
            }
        }
        let inv = enroll(a)?;
        binding(a, &inv.entries, enable_binding)?;
        focus(a, &inv, &inv.entries[0])?;
        // Unquoted $session IDs expand in a shell; print a copyable, safely quoted native command.
        println!(
            "Experimental native sidebar ready. Attach with:\ntmux -S {} attach-session -t {}\nF12 enabled: {}. No architecture approval recorded.",
            quote(a.explicit_socket_path()?),
            quote(&inv.session),
            !global(a, BINDING_KEY)?.is_empty()
        );
        Ok(())
    }
    fn focus(a: &Adapter, inv: &Inventory, e: &Entry) -> Result<()> {
        generation(a, &inv.snapshot.server)?;
        let pane = e
            .panel
            .as_ref()
            .context("Sidebar closed; run sidebar-show")?;
        ensure!(
            marked(a, pane.as_str())?,
            "Target is not a marked Sidebar pane"
        );
        // select-pane without -Z unzooms. Focus destination before making its window visible.
        let target = format!("{}:{}", inv.session, e.window.as_str());
        request(
            a,
            &[
                "select-pane",
                "-t",
                pane.as_str(),
                ";",
                "select-window",
                "-t",
                &target,
            ],
        )?;
        Ok(())
    }
    pub fn show(a: &Adapter, window: Option<&str>) -> Result<()> {
        let _lock = lock(a)?;
        let inv = enroll(a)?;
        binding(a, &inv.entries, false)?;
        let selected = match window {
            Some(w) => inv
                .entries
                .iter()
                .find(|e| e.window.as_str() == w)
                .context("Unknown enrolled window ID")?,
            None => &inv.entries[0],
        };
        focus(a, &inv, selected)
    }
    pub fn close(a: &Adapter, disable_binding: bool) -> Result<()> {
        let _lock = lock(a)?;
        let inv = inventory(a)?;
        if disable_binding {
            let owned = global(a, BINDING_KEY)?;
            if !owned.is_empty() {
                ensure!(
                    current_binding(a)? == owned,
                    "F12 was changed externally; leaving it untouched"
                );
                request(a, &["unbind-key", "-T", "root", "F12"])?;
                request(a, &["set-option", "-gu", BINDING_KEY])?;
            }
        }
        for e in inv.entries {
            if let Some(p) = e.panel {
                generation(a, &inv.snapshot.server)?;
                let fresh = inventory(a)?;
                ensure!(
                    fresh.entries.iter().any(|n| n.window == e.window
                        && n.panel.as_ref() == Some(&p)
                        && n.work != p),
                    "Sidebar target changed; no cleanup replayed"
                );
                ensure!(marked(a, p.as_str())?, "Refusing to kill an unmarked pane");
                request(a, &["kill-pane", "-t", p.as_str()])?;
            }
        }
        Ok(())
    }
    struct Screen;
    impl Drop for Screen {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
        }
    }
    pub fn pane(a: &Adapter) -> Result<()> {
        let own = PaneId::parse(
            &std::env::var("TMUX_PANE")
                .context("sidebar-pane must run inside its managed native pane")?,
        )?;
        let start = Instant::now();
        // Parent marks the new pane after split-window returns. A crashed parent cannot leave an unmarked shell.
        while !marked(a, own.as_str())? {
            ensure!(
                start.elapsed() < Duration::from_secs(3),
                "Sidebar enrollment marker never arrived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // Coordinate readers with enrollment so they never mistake an unmarked in-flight split for user work.
        let initial = {
            let _lock = lock(a)?;
            inventory(a)?
        };
        let source = initial
            .entries
            .iter()
            .find(|e| e.panel.as_ref() == Some(&own))
            .context("Sidebar not enrolled")?
            .window
            .clone();
        let expected = initial.snapshot.server.clone();
        enable_raw_mode()?;
        let _guard = Screen;
        execute!(io::stdout(), EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let mut highlighted = source.clone();
        let mut inv = initial;
        let mut last = Instant::now();
        let mut message = String::new();
        loop {
            terminal.draw(|f| {
                let areas =
                    Layout::vertical([Constraint::Min(2), Constraint::Length(7)]).split(f.area());
                let labels: Vec<_> = inv
                    .entries
                    .iter()
                    .map(|e| {
                        format!(
                            "{} {:?}{}",
                            e.window.as_str(),
                            e.name,
                            if e.window == source { " *" } else { "" }
                        )
                    })
                    .collect();
                let mut state = ListState::default()
                    .with_selected(inv.entries.iter().position(|e| e.window == highlighted));
                crate::sidebar::render_chooser_list(f, areas[0], &labels, &mut state);
                let hints = HintBar::new(
                    vec![
                        KeybindingInfo::new("j/k", "Browse"),
                        KeybindingInfo::new("Enter", "Other sidebar"),
                        KeybindingInfo::new("Tab/Esc", "Return to work"),
                        KeybindingInfo::new("q", "Close this sidebar"),
                    ],
                    "F12 Enter (opt-in)",
                );
                f.render_widget(hints, areas[1]);
                if !message.is_empty() {
                    f.render_widget(Paragraph::new(message.clone()), areas[1]);
                }
            })?;
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Release {
                        continue;
                    }
                    if key.code == KeyCode::Char('q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        return Ok(());
                    }
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                            let i = inv
                                .entries
                                .iter()
                                .position(|e| e.window == highlighted)
                                .unwrap_or(0);
                            let next = if matches!(key.code, KeyCode::Down | KeyCode::Char('j')) {
                                (i + 1).min(inv.entries.len() - 1)
                            } else {
                                i.saturating_sub(1)
                            };
                            highlighted = inv.entries[next].window.clone();
                            message.clear();
                        }
                        KeyCode::Enter | KeyCode::Tab | KeyCode::Esc => {
                            let result = (|| -> Result<()> {
                                let _lock = lock(a)?;
                                let fresh = inventory(a)?;
                                ensure!(
                                    fresh.snapshot.server.same_generation(&expected),
                                    "Server restarted; reopen sidebar"
                                );
                                if key.code == KeyCode::Enter {
                                    let e = fresh
                                        .entries
                                        .iter()
                                        .find(|e| e.window == highlighted)
                                        .context("Highlighted window disappeared")?;
                                    focus(a, &fresh, e)?;
                                } else {
                                    let e = fresh
                                        .entries
                                        .iter()
                                        .find(|e| {
                                            e.window == source && e.panel.as_ref() == Some(&own)
                                        })
                                        .context("Source sidebar disappeared")?;
                                    ensure!(
                                        !marked(a, e.work.as_str())?,
                                        "Working target is a sidebar"
                                    );
                                    request(a, &["select-pane", "-t", e.work.as_str()])?;
                                }
                                Ok(())
                            })();
                            if let Err(e) = result {
                                message = format!("{e:#}");
                            }
                        }
                        _ => {} // Unsupported commands are never forwarded to native work panes.
                    }
                }
            }
            if last.elapsed() >= Duration::from_millis(750) {
                let _lock = lock(a)?;
                inv = inventory(a)?;
                ensure!(
                    inv.snapshot.server.same_generation(&expected),
                    "Server restarted; no action replayed"
                );
                last = Instant::now();
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn binding_targets_only_marked_literal_ids() {
            let entries = vec![Entry {
                window: WindowId::parse("@0").unwrap(),
                work: PaneId::parse("%0").unwrap(),
                panel: Some(PaneId::parse("%1").unwrap()),
                name: "ignored; kill-server".into(),
            }];
            let command = binding_command(&entries).unwrap();
            assert!(command.contains("select-pane -t %1"));
            assert!(!command.contains("kill-server"));
            assert_eq!(quote("a'b"), "'a'\\''b'");
        }
    }
}
#[cfg(unix)]
pub use native::{close, demo, pane, show};
#[cfg(not(unix))]
mod unsupported {
    use crate::tmux::Adapter;
    use anyhow::{Result, bail};
    pub fn demo(_: &Adapter, _: &std::path::Path, _: bool) -> Result<()> {
        bail!("Native sidebar prototype requires Unix tmux")
    }
    pub fn show(_: &Adapter, _: Option<&str>) -> Result<()> {
        bail!("Native sidebar prototype requires Unix tmux")
    }
    pub fn close(_: &Adapter, _: bool) -> Result<()> {
        bail!("Native sidebar prototype requires Unix tmux")
    }
    pub fn pane(_: &Adapter) -> Result<()> {
        bail!("Native sidebar prototype requires Unix tmux")
    }
}
#[cfg(not(unix))]
pub use unsupported::{close, demo, pane, show};
