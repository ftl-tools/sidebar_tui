//! Sidebar TUI - A tmux-style session and window manager.
//!
//! This library provides the core components for a session and window manager (currently backed by Sidebar-owned PTYs):
//! - Terminal emulation via vt100
//! - PTY management via portable-pty
//! - Background server for persistence across TUI restarts
//! - State management for sidebar/terminal focus and modal UI

pub mod colors;
pub mod server;
pub mod env_capture;
pub mod hint_bar;
pub mod input;
pub mod input_handler;
pub mod name_generator;
pub mod pty;
pub mod sidebar;
pub mod state;
pub mod terminal;
pub mod updater;
pub mod tmux;
pub mod tmux_chooser;
pub mod tmux_sidebar;
