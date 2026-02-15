//! Sidebar TUI - A terminal session manager with session persistence.
//!
//! This library provides the core components for a terminal session manager:
//! - Terminal emulation via vt100
//! - PTY management via portable-pty
//! - Session daemon for persistence across TUI restarts
//! - State management for multi-pane focus and modal UI

pub mod colors;
pub mod daemon;
pub mod env_capture;
pub mod input;
pub mod input_handler;
pub mod pty;
pub mod sidebar;
pub mod state;
pub mod terminal;
