#!/usr/bin/env bash
# Swaps the live hledger finances journal for a canned demo one (so the
# Finances feature can be shown off without exposing real financial data),
# backing up the live journal first so it can be put back afterward. The
# app reads the journal file fresh on every request (no cache — see
# finances::list_spending_entries et al.), so no restart is needed either
# direction.
#
# Usage:
#   scripts/dev/demo_journal.sh            # back up live journal, install demo journal
#   scripts/dev/demo_journal.sh restore    # put the live journal back, remove demo journal
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEMO_JOURNAL="$PROJECT_DIR/scripts/dev/demo_finances.journal"

# Resolve the live journal path the same way app/src/config.rs does:
# config/local.toml's [finances] journal_path overrides config/default.toml's,
# both resolved relative to the project root (the app's WorkingDirectory
# under systemd — see deploy.sh).
resolve_journal_path() {
  local path=""
  if [[ -f "$PROJECT_DIR/config/local.toml" ]]; then
    path=$(awk '/^\[finances\]/{f=1;next} /^\[/{f=0} f && /^journal_path/' "$PROJECT_DIR/config/local.toml" \
      | sed -E 's/journal_path *= *"(.*)"/\1/' | head -1)
  fi
  if [[ -z "$path" ]]; then
    path=$(awk '/^\[finances\]/{f=1;next} /^\[/{f=0} f && /^journal_path/' "$PROJECT_DIR/config/default.toml" \
      | sed -E 's/journal_path *= *"(.*)"/\1/' | head -1)
  fi
  if [[ "$path" == /* ]]; then
    echo "$path"
  else
    echo "$PROJECT_DIR/$path"
  fi
}

JOURNAL_PATH="$(resolve_journal_path)"
BACKUP_PATH="${JOURNAL_PATH}.live_backup"
NO_LIVE_MARKER="${BACKUP_PATH}.no-live"

if [[ "${1:-}" == "restore" ]]; then
  if [[ -f "$NO_LIVE_MARKER" ]]; then
    rm -f "$JOURNAL_PATH" "$NO_LIVE_MARKER"
    echo "Removed demo journal — no live journal existed before the demo swap."
    exit 0
  fi
  if [[ ! -f "$BACKUP_PATH" ]]; then
    echo "No backup found at $BACKUP_PATH — nothing to restore (not currently in demo mode?)." >&2
    exit 1
  fi
  mv "$BACKUP_PATH" "$JOURNAL_PATH"
  echo "Restored live journal: $JOURNAL_PATH"
  exit 0
fi

if [[ -f "$BACKUP_PATH" || -f "$NO_LIVE_MARKER" ]]; then
  echo "Already in demo mode (backup already exists at $BACKUP_PATH)." >&2
  echo "Run '$0 restore' first before swapping in demo data again." >&2
  exit 1
fi

if [[ ! -f "$DEMO_JOURNAL" ]]; then
  echo "Demo journal not found at $DEMO_JOURNAL" >&2
  exit 1
fi

mkdir -p "$(dirname "$JOURNAL_PATH")"
if [[ -f "$JOURNAL_PATH" ]]; then
  cp "$JOURNAL_PATH" "$BACKUP_PATH"
else
  touch "$NO_LIVE_MARKER"
fi
cp "$DEMO_JOURNAL" "$JOURNAL_PATH"
echo "Demo journal installed at $JOURNAL_PATH"
echo "(live journal backed up to $BACKUP_PATH — run '$0 restore' to put it back)"
