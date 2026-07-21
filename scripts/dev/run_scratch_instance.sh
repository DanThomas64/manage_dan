#!/usr/bin/env bash
# Runs a second, fully isolated `app` instance alongside the real deployed
# one — its own port, its own scratch cwd/sqlite db, and its own nb notebook
# (nb notebooks are global, not scoped by cwd, so a distinct notebook name is
# what actually keeps it from touching real todo data; the scratch cwd/db
# just keeps `app.sqlite`/`printed_tasks`/etc. separate). Lets you manually
# exercise a change (e.g. hit http://localhost:<port>/todo/<id> after
# creating a task via the API) without risking the live systemd service's
# port, database, or `nb` notebooks — the trap this script avoids is exactly
# what bit an earlier debugging session: an ad hoc `cargo run -p app` from
# the repo cwd shares the real `app.sqlite` and default nb notebook with the
# live deployment, and a stale env var override silently not taking effect
# (rather than erroring) meant that run happened before it was caught.
#
# Usage: scripts/dev/run_scratch_instance.sh [port] [notebook]
#   port     — TCP port to listen on (default: 8099)
#   notebook — nb notebook name for todos (default: zz_scratch_test)
#
# Builds the app binary if needed, then runs it in the foreground; Ctrl+C to
# stop. Deletes its scratch nb notebook and scratch directory on exit.

set -euo pipefail

PORT="${1:-8099}"
NOTEBOOK="${2:-zz_scratch_test}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRATCH="$(mktemp -d -t manage_dan_scratch_XXXXXX)"

cleanup() {
  # Explicitly kill the app process rather than relying on it dying with the
  # script — a signal sent to this script's own PID (e.g. `kill <pid>`, not
  # Ctrl+C's process-group-wide SIGINT from an interactive terminal) does not
  # automatically propagate to a child process, so without this the app would
  # keep running as an orphan after the script exits.
  if [ -n "${APP_PID:-}" ] && kill -0 "$APP_PID" 2>/dev/null; then
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  nb notebooks delete "$NOTEBOOK" --force >/dev/null 2>&1 || true
  rm -rf "$SCRATCH"
}
trap cleanup EXIT INT TERM

mkdir -p "$SCRATCH/config"
cp "$REPO_ROOT/config/default.toml" "$SCRATCH/config/default.toml"
cat > "$SCRATCH/config/local.toml" <<EOF
[printer]
mode = "terminal"

[todo]
nb_notebook = "$NOTEBOOK"
EOF

echo "Building app (debug profile)..."
(cd "$REPO_ROOT" && cargo build -p app) >&2

echo ""
echo "Scratch instance starting:"
echo "  API:        http://127.0.0.1:$PORT/api/v1"
echo "  QR/task pg: http://127.0.0.1:$PORT/todo/<id>"
echo "  nb notebook: $NOTEBOOK (deleted on exit)"
echo "  scratch dir: $SCRATCH (deleted on exit)"
echo "  printer mode: terminal (no physical prints)"
echo ""

cd "$SCRATCH"
APP_CONFIG_DIR="$SCRATCH/config" APP_API_PORT="$PORT" "$REPO_ROOT/target/debug/app" &
APP_PID=$!
wait "$APP_PID"
