# michi

> A native Rust control panel for running multiple Claude Code instances in parallel,
> each in its own isolated `git worktree`.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Status](https://img.shields.io/badge/status-alpha-orange)](#status)
[![CI](https://github.com/kmilo93sd/michi/actions/workflows/ci.yml/badge.svg)](https://github.com/kmilo93sd/michi/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-blue.svg)](https://www.rust-lang.org)

## What

If you run 3+ Claude Code instances at the same time, they stomp on each other's
git state. michi solves that. Each "job" gets its own git worktree on its own
branch and (eventually) its own embedded Claude Code terminal. One window. One
sidebar. Switch between jobs without losing context.

Built native in Rust with [egui](https://github.com/emilk/egui) — no Electron,
no web view. Cross-platform (Windows, macOS, Linux) from day one.

## Status

**Alpha · active development.** The current build renders a tree of workspaces →
repos → jobs with mock data, a configurable theme, hover/selection states and
empty-state UX. The git worktree integration and the embedded Claude Code
terminal are next. See [Roadmap](#roadmap).

## Features

- **Three-level tree**: workspaces → repos → jobs (paralelizable instances)
- **Status dots per job** (idle, thinking, paused, error, needs attention)
- **AI-friendly theme** via `~/.michi/theme.toml` — colors, fonts and spacings
  editable by hand or by Claude itself
- **Dark mode by default** with a sober power-user palette
- **Monospace sidebar**, full-width clickable rows, tree lines, cursor pointers,
  proper hover/selection backgrounds
- **Empty state** that explains the model the first time you open the app
- **Cross-platform** — Windows, macOS, Linux from day one (no platform-specific
  code in the codebase)

## Roadmap

| Phase | Description | Status |
|------|-------------|--------|
| 1 | Bootstrap (`cargo run` opens window, tracing, modules) | DONE |
| 2 | Static layout with mock data (tree, cards, theme, empty state) | DONE |
| 3 | Real `git worktree` create/remove + state persistence (`~/.michi/state.json`) | NEXT |
| 4 | Embedded Claude Code terminal (`portable-pty` + `egui_term`) | |
| 5 | Git actions from the UI (diff, commit & push, open folder) | |
| 6 | Polish + dogfood + keyboard shortcuts | |
| V1 | Discovery of workspaces, CLAUDE.md parsing, Docker integration, port auto-assignment | |
| V2 | Local LLM (gemma) for PTY analysis, MCP gating, skill management, shared memory between Claude Codes | |

See the [spec](https://github.com/kmilo93sd/lelemon-workspace/tree/master/specs/20260517-1716-michi-poc)
(private workspace) for full design notes.

## Quickstart

Requirements:

- Rust 1.95+ (`rustup install stable`)
- A C/C++ toolchain (MSVC Build Tools on Windows, Xcode CLI Tools on macOS,
  `build-essential` on Linux)
- Git

Build and run:

```bash
git clone https://github.com/kmilo93sd/michi.git
cd michi
cargo run
```

On first launch michi creates `~/.michi/theme.toml` with the default dark
palette. Edit it (or ask Claude to edit it) and reopen the app to apply changes.

## Configuration

All colors, fonts and panel sizes live in `~/.michi/theme.toml`. Colors are
plain hex strings — easy for humans, easy for AI:

```toml
accent = "#f7c948"
bg_base = "#0f0f11"
bg_surface = "#16161a"
bg_card_selected = "#26262c"
status_idle = "#30d158"
status_thinking = "#f7c948"
font_mono_size = 13.0
sidebar_default_width = 300.0
card_row_height = 56.0
```

If a value is missing or invalid the app logs a warning and falls back to the
default. Future versions will hot-reload the file with [`notify`](https://docs.rs/notify).

## Development

```bash
# Run with auto-reload on every save
cargo install cargo-watch
cargo watch -c -x run

# Lint and format gates (must pass before commit)
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# Tests
cargo test
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full contributor guide.

## Tech stack

- [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/master/crates/eframe)
  for the immediate-mode GUI
- [tokio](https://tokio.rs) for the async runtime
- [tracing](https://docs.rs/tracing) for structured logging (rolling file
  appender in `~/.michi/logs/`)
- [anyhow](https://docs.rs/anyhow) for error context
- [serde](https://serde.rs) + [toml](https://docs.rs/toml) for the
  human-editable config
- `portable-pty` + [`egui_term`](https://github.com/Harzu/egui_term) for the
  embedded terminal (Phase 4)

## License

[MIT](LICENSE) © 2026 Camilo Alaniz
