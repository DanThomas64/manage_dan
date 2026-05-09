# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo build -p <crate>         # Build specific crate
cargo check                    # Type-check without building
cargo test                     # Run all tests
cargo test -p <crate>          # Test specific crate
cargo test <test_name>         # Run a single test by name
cargo clippy                   # Lint
cargo fmt                      # Format code
cargo fmt -- --check           # Check formatting only
cargo run                      # Run the app
```

## Architecture

This is a Rust Cargo workspace with one binary (`app`) and library crates: `db`, `log`, `printer`, `todo`, `vikunja`, `lists`, `notes`, `project`, plus a second binary `tui`. The `app` crate depends on all library crates.

**Execution flow**: `main()` loads `AppConfig`, calls `SystemsStatus::init()` which sequentially calls `init()` on each subsystem crate in dependency order (db → log → notes → project → printer → todo → lists), tracking per-subsystem `Status` (Go/Nogo/Degraded/Unknown/Init). `SystemsGoNogo::calculate_initial_status()` derives overall health; `start_monitoring()` spawns a tokio task that loops every 500ms updating it. After init, the app prints a startup receipt, spawns background tasks (print monitor, daily summary, completed summary), then starts the warp HTTP API server.

**Standardized module pattern** — every library crate follows the same structure:
- `lib.rs`: `init() -> Result<(), Error>` and tests
- `{crate}_error.rs`: custom error type via `thiserror`
- `{crate}_prelude.rs`: re-exports for ergonomic imports

**Shared workspace dependencies** (declared in root `Cargo.toml`, inherited with `workspace = true`): `tokio` (full, async runtime), `tracing` (structured logging), `thiserror` (custom errors), `anyhow` (error context), `chrono` (date/time).

**Logging**: `tracing-subscriber` configured in `app/src/main.rs` with RFC 3339 timestamps, line numbers, ANSI disabled, DEBUG level.
