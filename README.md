<p align="center">
  <img src="assets/logo.svg" width="120" alt="michi logo" />
</p>

<h1 align="center">michi</h1>

<p align="center">
  A native harness for running many AI coding agents in parallel on one machine.
  michi watches every Claude Code session, shows what each one is doing and what
  it's using, and keeps them from stepping on each other.
</p>

<p align="center">
  Not an IDE, not a cloud orchestrator. michi doesn't edit code — Claude does.
  It's the traffic-control tower for your parallel Claude sessions: git, ports,
  databases and containers, isolated per session.
</p>

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Status](https://img.shields.io/badge/status-alpha-orange)](#status)
[![CI](https://github.com/kmilo93sd/michi/actions/workflows/ci.yml/badge.svg)](https://github.com/kmilo93sd/michi/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-blue.svg)](https://www.rust-lang.org)

## What

Run several Claude Code sessions at once and they collide: same port `8080`,
duplicate Docker stacks, clobbered git state — and you have no idea which session
is eating your RAM or still holding a port. michi is the control panel that fixes
this. It **observes** every session and **isolates** its resources.

Two kinds of session:

- **Managed** — michi launched it. It has an embedded terminal (PTY) and full
  control: inject prompts, assign ports, isolate its database, sandbox it in a
  container.
- **Detected** — running outside michi (your terminal, VS Code). michi sees it by
  scanning the host, read-only, until you *bring it in*.

Built native in Rust with [egui](https://github.com/emilk/egui) — no Electron,
no web view, no server. Cross-platform (Windows, macOS, Linux) from day one.

## Status

**Alpha · active development.** The POC foundation is done (workspace → repo →
session tree, git worktrees, embedded terminal, persistence). On top of it, michi
now detects every Claude session on the host, shows a per-session process tree
with aggregate RAM and real status (busy / idle / waiting), and allocates ports
per session. Next up is the V1 harness: prompt injection, port-conflict
detection, database isolation and the container sandbox. See [Roadmap](#roadmap).

## Features

- **Detects every Claude session** on the host (managed + external), grouped by
  workspace
- **Per-session resource tree** — processes, aggregate RAM, classified into
  shells / runtimes / docker
- **Real status per session** (busy, idle, waiting) read from Claude's own
  session files
- **Per-session port allocation** — `PORT_*` detected from `.env` and injected as
  env vars so parallel sessions don't collide
- **Embedded terminal** for managed sessions (`portable-pty` + `egui_term`)
- **Workspace → repo → session tree** with status dots, hover/selection states
  and an empty state that explains the model
- **AI-friendly theme** via `~/.michi/theme.toml` — colors, fonts and spacings
  editable by hand or by Claude itself
- **Cross-platform** — Windows, macOS, Linux from day one (no platform-specific
  code without a fallback)

## Roadmap

michi grows by **graceful degradation**: Level 0 (observe) works on any project
with zero setup; higher levels kick in when the project allows it.

| Stage | What | Status |
|------|------|--------|
| Foundation | Native window, workspace tree, theme, git worktrees, embedded terminal, persistence | Done |
| Observability (L0) | Detect all sessions, process tree, RAM, real status, ports | Done / in progress |
| Coordination | Inject prompts into managed sessions, real LISTEN ports, port-conflict detection + "fix with Claude" | Next |
| DB isolation (L2–3) | One shared Postgres, ephemeral DB/schema per session, `DATABASE_URL` injected | Planned |
| Container sandbox | Per-session container (worktree mounted, port proxy), **native fallback when Docker is absent** | Planned |
| Unified observability | Centralized per-session log streams, keyboard shortcuts, dogfood | Planned |

**Direction (2026-05-21): container-first.** Sessions michi launches run in a
container sandbox when Docker is available, with a native fallback when it isn't
— Docker is *preferred, not required*. See the
[spec](specs/20260517-1716-michi-poc/SPEC.md) for the full design notes.

## Quickstart

Requirements:

- Rust 1.95+ (`rustup install stable`)
- A C/C++ toolchain (MSVC Build Tools on Windows, Xcode CLI Tools on macOS,
  `build-essential` on Linux)
- Git
- Docker (optional) — enables the container sandbox and shared infra; without it
  michi runs sessions natively

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

Working on michi:

- [CLAUDE.md](CLAUDE.md) — instructions for AI agents and the contract this
  repo holds itself to (TDD, clean code, design system, gotchas).
- [CONTRIBUTING.md](CONTRIBUTING.md) — setup, code style, PR flow.
- [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md) — tokens, UI patterns, egui gotchas.

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
  embedded terminal of managed sessions
- [`sysinfo`](https://docs.rs/sysinfo) for the per-session process tree and RAM
  accounting

## License

[MIT](LICENSE) © 2026 Camilo Alaniz
