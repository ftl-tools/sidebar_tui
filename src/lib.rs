//! Sidebar TUI - A terminal session manager with session persistence.
//!
//! This library provides the core components for a terminal session manager:
//! - Terminal emulation via vt100
//! - PTY management via portable-pty
//! - Session daemon for persistence across TUI restarts

pub mod daemon;
pub mod env_capture;
pub mod input;
pub mod pty;
pub mod terminal;
