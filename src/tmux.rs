//! Read-only tmux preview. No legacy runtime or terminal rendering is used here.
use anyhow::{Context, Result, bail, ensure};
use clap::{Args, Subcommand};
use serde::Serialize;
use std::{collections::BTreeMap, process::Command};

#[derive(Debug, Args)]
pub struct TmuxCli {
    /// Explicit tmux socket name (conflicts with --socket-path)
    #[arg(long, global = true, conflicts_with = "socket_path")]
    pub socket_name: Option<String>,
    /// Explicit tmux socket path; otherwise use $TMUX, then tmux's default
    #[arg(long, global = true)]
    pub socket_path: Option<String>,
    /// Machine-readable output (names are escaped)
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub action: Inspect,
}

#[derive(Debug, Subcommand)]
pub enum Inspect {
    /// Diagnose tmux version, connection and read-only format capabilities
    Doctor,
    /// List sessions without starting a server
    ListSessions,
    /// List windows and their session memberships
    ListWindows,
    /// List native panes (linked windows are deduplicated)
    ListPanes,
}

macro_rules! id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        pub struct $name(String);
        impl $name {
            fn parse(value: &str) -> Result<Self> {
                ensure!(
                    value.starts_with($prefix)
                        && value.len() > 1
                        && value[1..].bytes().all(|b| b.is_ascii_digit()),
                    "Invalid tmux {}: {:?}",
                    stringify!($name),
                    value
                );
                Ok(Self(value.into()))
            }
        }
    };
}
id!(SessionId, '$');
id!(WindowId, '@');
id!(PaneId, '%');

/// IDs below are valid only within this observation, never durable disk keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerIdentity {
    pub socket: String,
    pub pid: u64,
    pub started: u64,
    pub observation: String,
}
#[derive(Debug, Serialize)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Window {
    pub id: WindowId,
    pub name: String,
}
#[derive(Debug, Serialize)]
pub struct Membership {
    pub session: SessionId,
    pub window: WindowId,
    pub index: u64,
    pub active: bool,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Pane {
    pub id: PaneId,
    pub window: WindowId,
    pub title: String,
    pub active: bool,
    pub pid: u64,
}
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub server: ServerIdentity,
    pub sessions: Vec<Session>,
    pub windows: Vec<Window>,
    pub memberships: Vec<Membership>,
    pub panes: Vec<Pane>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Socket {
    Name(String),
    Path(String),
    Default,
}
impl Socket {
    pub fn resolve(
        name: Option<String>,
        path: Option<String>,
        context: Option<String>,
    ) -> Result<Self> {
        ensure!(
            name.is_none() || path.is_none(),
            "Use only one socket selector"
        );
        if let Some(n) = name {
            ensure!(
                !n.is_empty() && !n.contains('/'),
                "Socket name must be nonempty and contain no slash"
            );
            return Ok(Self::Name(n));
        }
        if let Some(p) = path {
            ensure!(!p.is_empty(), "Socket path is empty");
            return Ok(Self::Path(p));
        }
        if let Some(c) = context.filter(|c| !c.is_empty()) {
            // Split from the right: socket paths themselves may contain commas.
            let fields: Vec<_> = c.rsplitn(3, ',').collect();
            ensure!(
                fields.len() == 3
                    && !fields[2].is_empty()
                    && fields[..2]
                        .iter()
                        .all(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())),
                "Malformed TMUX context; pass --socket-path or --socket-name explicitly"
            );
            return Ok(Self::Path(fields[2].into()));
        }
        Ok(Self::Default)
    }
}

pub struct Adapter {
    socket: Socket,
}
impl Adapter {
    pub fn new(socket: Socket) -> Self {
        Self { socket }
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new("tmux");
        // Never allow tmux's server-start behavior or inherited context to override selection.
        // Non-UTF-8 clients replace name bytes with underscores, invalidating lengths.
        cmd.args(["-N", "-u"]).env_remove("TMUX").env("LC_ALL", "C");
        match &self.socket {
            Socket::Name(n) => {
                cmd.args(["-L", n]);
            }
            Socket::Path(p) => {
                cmd.args(["-S", p]);
            }
            Socket::Default => {}
        }
        cmd
    }
    fn run(&self, args: &[&str]) -> Result<String> {
        let out = self.command().args(args).output().context(
            "Cannot execute tmux. Install tmux (preview tested with 3.6a), ensure it is on PATH; ordinary sb still uses the legacy backend")?;
        ensure!(
            out.status.success(),
            "tmux read-only request failed: {}. Check the socket/permissions and start a session explicitly with tmux if needed; no server was created",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        String::from_utf8(out.stdout)
            .context("tmux returned non-UTF-8 output; inventory was discarded")
    }
    pub fn version(&self) -> Result<String> {
        let version = self.run(&["-V"])?;
        // Only the local baseline has been tested; do not invent a minimum version claim.
        ensure!(
            version.trim() == "tmux 3.6a",
            "Unsupported tmux version {:?}; this preview is validated only on tmux 3.6a",
            version.trim()
        );
        Ok(version.trim().into())
    }
    fn identity(&self) -> Result<(String, u64, u64)> {
        let rows = self.rows("display-message", &["socket_path", "pid", "start_time"])?;
        ensure!(rows.len() == 1, "Missing server identity/capability");
        Ok((rows[0][0].clone(), rows[0][1].parse()?, rows[0][2].parse()?))
    }
    fn rows(&self, command: &str, fields: &[&str]) -> Result<Vec<Vec<String>>> {
        // Shell quoting does not escape tabs/newlines in tmux formats. Byte-length framing
        // preserves arbitrary UTF-8 names, including delimiters and control characters.
        let format: String = fields
            .iter()
            .map(|f| format!("#{{n:{f}}}:#{{{f}}}"))
            .collect();
        let args = match command {
            "display-message" => vec![command, "-p", &format],
            "list-sessions" => vec![command, "-F", &format],
            _ => vec![command, "-a", "-F", &format],
        };
        parse_rows(&self.run(&args)?, fields.len())
            .with_context(|| format!("Unsupported {command} inventory format"))
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.version()?;
        let before = self.identity()?;
        // display-message with the session loop works even on an explicitly kept empty server.
        let session_count = self.run(&["display-message", "-p", "#{S:1}"])?;
        let empty = session_count.trim().is_empty();
        let sessions = if empty {
            vec![]
        } else {
            self.rows("list-sessions", &["session_id", "session_name"])?
                .into_iter()
                .map(|r| {
                    Ok(Session {
                        id: SessionId::parse(&r[0])?,
                        name: r[1].clone(),
                    })
                })
                .collect::<Result<Vec<_>>>()?
        };
        let mut windows = BTreeMap::new();
        let mut memberships = vec![];
        let mut panes = BTreeMap::new();
        if !empty {
            for r in self.rows(
                "list-windows",
                &[
                    "session_id",
                    "window_id",
                    "window_index",
                    "window_active",
                    "window_name",
                ],
            )? {
                let session = SessionId::parse(&r[0])?;
                let window = WindowId::parse(&r[1])?;
                ensure!(
                    sessions.iter().any(|s| s.id == session),
                    "Inventory changed; retry inspection"
                );
                let record = Window {
                    id: window.clone(),
                    name: r[4].clone(),
                };
                if let Some(old) = windows.insert(window.clone(), record) {
                    ensure!(old.name == r[4], "Window changed during inspection; retry");
                }
                memberships.push(Membership {
                    session,
                    window,
                    index: r[2].parse()?,
                    active: boolean(&r[3])?,
                });
            }
            for r in self.rows(
                "list-panes",
                &[
                    "pane_id",
                    "window_id",
                    "pane_title",
                    "pane_active",
                    "pane_pid",
                ],
            )? {
                let pane = Pane {
                    id: PaneId::parse(&r[0])?,
                    window: WindowId::parse(&r[1])?,
                    title: r[2].clone(),
                    active: boolean(&r[3])?,
                    pid: r[4].parse()?,
                };
                ensure!(
                    windows.contains_key(&pane.window),
                    "Inventory changed; retry inspection"
                );
                if let Some(old) = panes.insert(pane.id.clone(), pane) {
                    ensure!(
                        panes.get(&old.id) == Some(&old),
                        "Pane changed during inspection; retry"
                    );
                }
            }
        }
        ensure!(
            before == self.identity()?,
            "tmux server changed during inspection; discard IDs and retry"
        );
        Ok(Snapshot {
            server: ServerIdentity {
                socket: before.0,
                pid: before.1,
                started: before.2,
                observation: format!("{:032x}", rand::random::<u128>()),
            },
            sessions,
            windows: windows.into_values().collect(),
            memberships,
            panes: panes.into_values().collect(),
        })
    }
}
fn boolean(s: &str) -> Result<bool> {
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => bail!("Unsupported boolean format: {s:?}"),
    }
}
fn parse_rows(text: &str, count: usize) -> Result<Vec<Vec<String>>> {
    let mut rest = text;
    let mut rows = vec![];
    while !rest.is_empty() {
        let mut row = vec![];
        for _ in 0..count {
            let (length, value) = rest
                .split_once(':')
                .context("Unsupported tmux length format")?;
            ensure!(
                !length.is_empty() && length.bytes().all(|b| b.is_ascii_digit()),
                "Invalid field length"
            );
            let n: usize = length.parse()?;
            let field = value.get(..n).context("Truncated or invalid UTF-8 field")?;
            row.push(field.into());
            rest = &value[n..];
        }
        rest = rest
            .strip_prefix('\n')
            .context("Invalid tmux record boundary")?;
        rows.push(row);
    }
    Ok(rows)
}

pub fn run(cli: TmuxCli) -> Result<()> {
    let adapter = Adapter::new(Socket::resolve(
        cli.socket_name,
        cli.socket_path,
        std::env::var("TMUX").ok(),
    )?);
    let snapshot = adapter.snapshot()?;
    let value = match cli.action {
        Inspect::Doctor => {
            serde_json::json!({"version": "tmux 3.6a", "read_only": true, "capabilities": "length-framed inventory verified", "inventory": snapshot})
        }
        Inspect::ListSessions => {
            serde_json::json!({"server": snapshot.server, "sessions": snapshot.sessions})
        }
        Inspect::ListWindows => {
            serde_json::json!({"server": snapshot.server, "windows": snapshot.windows, "memberships": snapshot.memberships})
        }
        Inspect::ListPanes => {
            serde_json::json!({"server": snapshot.server, "panes": snapshot.panes})
        }
    };
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("tmux preview — read-only; tmux 3.6a (IDs scoped to this observation)");
        let inventory = value.get("inventory").unwrap_or(&value);
        println!("Server: {}", inventory["server"]);
        for kind in ["sessions", "windows", "memberships", "panes"] {
            if let Some(records) = inventory[kind].as_array() {
                println!("{kind} ({}):", records.len());
                for record in records {
                    // JSON string quoting prevents names/titles from injecting terminal controls.
                    let fields = record
                        .as_object()
                        .expect("inventory record")
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<_>>()
                        .join("  ");
                    println!("  {fields}");
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_preserves_names() {
        let name = "é🙂\t\n:\\#{pane_id}\u{1b}";
        let wire = format!("2:@0{}:{}\n0:2:$1\n", name.len(), name);
        assert_eq!(
            parse_rows(&wire, 2).unwrap(),
            vec![vec!["@0", name], vec!["", "$1"]]
        );
        for bad in ["1:é\n", "99:x\n", "a:x\n", "1:x", "1:xjunk\n"] {
            assert!(parse_rows(bad, 1).is_err());
        }
    }
    #[test]
    fn typed_targets_and_socket_scope() {
        assert!(SessionId::parse("$0").is_ok());
        for bad in ["@0", "$", "$-1", "$0;kill-server"] {
            assert!(SessionId::parse(bad).is_err());
        }
        assert!(WindowId::parse("@42").is_ok());
        assert!(PaneId::parse("%9").is_ok());
        assert_eq!(
            Socket::resolve(None, None, Some("/tmp/a,b,123,0".into())).unwrap(),
            Socket::Path("/tmp/a,b".into())
        );
        assert_eq!(
            Socket::resolve(Some("isolated".into()), None, Some("bad".into())).unwrap(),
            Socket::Name("isolated".into())
        );
        assert!(Socket::resolve(Some("x".into()), Some("y".into()), None).is_err());
        assert!(Socket::resolve(None, None, Some("bad".into())).is_err());
        assert!(boolean("").is_err());
    }
}
