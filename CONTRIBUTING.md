# Contributing to michi

Thanks for considering a contribution. michi is an early-stage POC, so the bar
is "make it work, keep it clean, justify the choice." Pull requests of any
size are welcome.

## Quick start

```bash
git clone https://github.com/kmilo93sd/michi.git
cd michi
cargo run
```

For an auto-reload dev loop:

```bash
cargo install cargo-watch
cargo watch -c -x run
```

## Pre-commit gates

These must pass green before any commit lands on `main`:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs all three on Linux, macOS and Windows. A PR that fails CI will not be
merged.

## TDD is mandatory

Every change in this repo follows **red-green-refactor**:

1. **RED.** Write the failing test first, before touching production code. If
   you are fixing a bug, the new test reproduces the bug and fails.
2. **GREEN.** Write the minimum code that makes the test pass. No speculative
   abstractions, no features beyond what the test exercises.
3. **REFACTOR.** Clean up while keeping every test green.

Derived rules:

- **Every PR adds at least one new test.** No exceptions, bugfixes included.
- **No commits with broken tests.** `cargo test --all-targets` must pass.
- **Tests come before production code, not after.** The order is the whole
  point of TDD.
- Reasonable exception: pure-visual UI changes (theme tokens, spacings) where
  there is no logic to assert. These go through manual visual review on the
  PR screenshot.

If you are an AI agent working on this repo: re-read [CLAUDE.md](./CLAUDE.md)
section 1 before opening any PR.

## Code style

These rules are not negotiable. They exist to keep the codebase readable for
both humans and AI assistants.

- **No `#[allow(dead_code)]` or similar warning silencers.** If a field is
  unused, use it or remove it. If an API is deprecated, migrate. If an import
  is unused, drop it. Hiding warnings is technical debt.
- **No `unwrap()` or `expect()` outside of `#[cfg(test)]`** (the `main`
  bootstrap is the only exception, and we try to keep that minimal too).
- **`anyhow::Result<T>`** for fallible functions. Attach context with
  `.with_context(|| ...)` before propagating.
- **No `unsafe`** in this POC.
- **Cross-platform paths.** Use `dirs::home_dir()` and `PathBuf::join`. Never
  hardcode `\\` or `/` separators.
- **Avoid `Arc<Mutex<T>>` "just in case."** Use it only when state is actually
  shared between threads.
- **Document non-obvious clones.** A `.clone()` to dodge a borrow checker
  acrobat is fine in this POC, but leave a one-line comment explaining why.
- **One concept per file.** When a file grows past ~300 lines, consider
  splitting it.

## Architecture notes

- `src/app.rs` — the `App` struct holding global state and the `eframe::App`
  impl. UI rendering happens here.
- `src/theme.rs` — colors, fonts and spacings. Loaded from
  `~/.michi/theme.toml`. **This is the only place where `Color32::from_rgb`
  lives.**
- `src/state/` — `Job`, `JobStatus`, `Workspace`, `Repo` and (eventually)
  persistence.
- `src/git/` — git operations (worktree, status, diff). Currently a stub.
- `src/terminal/` — embedded terminal integration (Phase 4). Currently a stub.
- `src/ui/` — reusable widgets. Currently a stub.

## Theme is data, not code

`~/.michi/theme.toml` is the contract with users (and with Claude Code itself).
Every colour, font size and spacing lives there. When you add a new
configurable value to the UI, add a field to the `Theme` struct in
`src/theme.rs` and serialise it in `ThemeConfig`. Don't hardcode values in
`app.rs`.

## Pull request flow

1. Fork or create a feature branch from `main` (e.g. `feature/3e-modal-new-job`).
2. Implement the change in small, reviewable commits.
3. Run the three gates locally (`fmt`, `clippy`, `test`).
4. Open the PR using the template. Describe what changed and why. Attach a
   screenshot if it affects the UI.
5. Address review comments. Merging is up to the maintainer.

## Reporting bugs

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md). Include
your OS, Rust version (`rustc --version`) and the contents of
`~/.michi/logs/michi.log` if it might be relevant.

## License

By contributing you agree that your contributions will be licensed under the
[MIT License](LICENSE).
