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
cargo run -p app                # Run the backend server (http://0.0.0.0:8080)
cargo run -p tui                # Run the TUI (needs the server running; reads MANAGE_API_URL, default http://127.0.0.1:8080)
./deploy.sh                     # Build release + install as a native systemd service + nginx reverse proxy
```

Local config lives in `config/local.toml` (gitignored, layered over `config/default.toml`); any setting can also be overridden via an `APP_`-prefixed env var (e.g. `APP_PRINTER_MODE=usb`).

## Architecture

This is a Rust Cargo workspace with one binary (`app`) and library crates: `db`, `log`, `printer`, `todo`, `vikunja`, `lists`, `notes`, `project`, plus a second binary `tui`. The `app` crate depends on all library crates. `frontend/` (vanilla JS SPA, served by nginx) and `android/` are separate, non-Cargo clients of the HTTP API.

**Execution flow**: `main()` (`app/src/main.rs`) loads `AppConfig`, calls `SystemsStatus::init()` (`app/src/nogo.rs`) which sequentially calls `init()` on each subsystem crate in dependency order (db → log → notes → project → printer → todo → lists), tracking per-subsystem `Status` (Go/Nogo/Degraded/Unknown/Init). DB is initialized first so the log table exists before logging starts. `notes::init()` checks for the `nb` CLI binary; the notes subsystem goes Nogo if `nb` is not installed. `SystemsGoNogo::calculate_initial_status()` derives overall health from the fold of all subsystem statuses; `start_monitoring()` spawns a tokio task that recomputes it every 500ms. After init, the app prints a startup receipt to the configured printer (todo/list stats fetched concurrently via `tokio::join!`, with failures rendered as placeholder text rather than aborting startup), spawns background tasks (Vikunja print monitor, daily summary, end-of-day completed summary — each printed once immediately if not already run today, then rescheduled to their configured hour), then starts the `warp` HTTP API server (`app/src/api.rs`) at `/api/v1/...`.

**Standardized module pattern** — every library crate follows the same structure:
- `lib.rs`: `init() -> Result<(), Error>` and tests
- `{crate}_error.rs`: custom error type via `thiserror`
- `{crate}_prelude.rs`: re-exports for ergonomic imports

`app/src/error.rs` defines `AppError`, which wraps every subsystem's error type (`#[from] DbLibError`, `NotesLibError`, etc.) via `thiserror`'s `#[error(transparent)]`, so subsystem errors surface at the top level without manual conversion.

**Shared workspace dependencies** (declared in root `Cargo.toml`, inherited with `workspace = true`): `tokio` (full, async runtime), `tracing` (structured logging), `thiserror` (custom errors), `anyhow` (error context), `chrono` (date/time), `serde`, `config` (layered TOML + env config), `escpos` (thermal printer), `reqwest` (Vikunja HTTP client), `uuid`.

**Logging**: `tracing-subscriber` configured in `app/src/main.rs` with RFC 3339 timestamps, line numbers, ANSI disabled, DEBUG level.

**Notes subsystem**: notes are not stored in the app's own DB — they're markdown files managed by the external [`nb`](https://xwmx.github.io/nb/) CLI, with full-text search shelled out to `nb search`. If files are edited outside `nb`, its index must be resynced with `nb index reconcile`. `nb_client.rs` always addresses notebooks with an explicit `notebook:cmd` prefix, including `home` — `nb` persists whichever notebook a colon-prefixed command last targeted as its own "current" notebook, so a bare/unprefixed command silently drifts onto that instead of `home`.

**Log feature**: a dedicated `log` notebook, driven by `nb daily`, backs `POST`/`GET /api/v1/notes/daily`. Each entry is appended to that day's file under an auto-generated `## HH:MM:SS` heading, body-formatted with the same title/tags/content layout regular notes use (`nb_client::format_note_body`). `notes::recent_logs()` re-parses those multi-entry day-files back into individual entries for the last N days. The `log` notebook is excluded from the general notes browsing surfaces (`folders`/`list`/`search`) so daily-log files don't clutter regular note browsing.

**Frontend** (`frontend/index.html`, single-file vanilla JS): four full-screen sections — Home (landing page), Lists, Notes, Log — each an `.app-overlay` div toggled by `showView()`, with exactly one `.active` at a time. Home is the default view; every other section has its own "Home" button to get back. Lists' overlay wraps the original header+sidebar+panel markup unchanged.

**Deployment**: dev and prod are the same machine — no Docker, no cross-compilation. `deploy.sh` builds `app` natively (`cargo build --release -p app`), installs it to `/usr/local/bin/manage_dan`, and installs/restarts a `manage_dan` systemd unit (`WorkingDirectory` = project root, so relative paths like `app.sqlite` and `data/logs/app.log` resolve the same as under `cargo run`) plus an nginx reverse proxy (serves `frontend/index.html`, proxies `/api/` and `/todo/` to `127.0.0.1:8080`). See the script's header comments for one-time setup (nginx, `nb`, `plugdev` group for USB printer access).
