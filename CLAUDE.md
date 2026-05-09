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

This is a Rust Cargo workspace with one binary (`app`) and six library crates: `db`, `log`, `tasks`, `todo`, `notes`, `project`. The `app` crate depends on all of them.

**Execution flow**: `main()` initializes tracing, calls `SystemsStatus::init()` which sequentially calls `init()` on each subsystem crate, tracking per-subsystem `Status` (Go/Nogo/Degraded/Unknown/Init). A tokio task (`SystemsGoNogo`) then loops every 500ms logging overall health — any `Nogo` subsystem makes the overall status `Nogo`.

**Standardized module pattern** — every library crate follows the same structure:
- `lib.rs`: `init() -> Result<(), Error>` and tests
- `{crate}_error.rs`: custom error type via `thiserror`
- `{crate}_prelude.rs`: re-exports for ergonomic imports

**Shared workspace dependencies** (declared in root `Cargo.toml`, inherited with `workspace = true`): `tokio` (full, async runtime), `tracing` (structured logging), `thiserror` (custom errors), `anyhow` (error context), `chrono` (date/time).

**Logging**: `tracing-subscriber` configured in `app/src/main.rs` with RFC 3339 timestamps, line numbers, ANSI disabled, DEBUG level.
