//! michi — control panel for parallel Claude Code instances in git worktrees.
//!
//! This crate exposes the library API used by the `michi` binary. Splitting
//! `lib.rs` from `main.rs` is the idiomatic Rust pattern: the binary stays
//! thin, every public function is part of the documented API and gets
//! exercised through tests instead of going through "dead code" purgatory.

pub mod app;
pub mod claude_config;
pub mod git;
pub mod port_detector;
pub mod state;
pub mod system;
pub mod terminal;
pub mod theme;
pub mod ui;
pub mod worker;
pub mod workspace_prep;
