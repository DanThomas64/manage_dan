#!/usr/bin/env bash
# Minimal repro for a bug in the installed `nb` CLI itself (confirmed present
# through v7.25.4, the latest published release as of 2026-07): its top-level
# argument dispatcher mishandles `move`/`rename` when invoked as bare `move`
# plus a separate `notebook:id` source selector (the shape this repro uses,
# and the shape `project/tests/archive_restore_delete.rs` used to fail
# against) — it hits an internal "Not found" error before ever reaching the
# actual file move. This is now WORKED AROUND in the app itself: every
# `nb move`/`rename` call in this codebase (`notes/src/nb_client.rs`,
# `todo/src/backends/nb.rs`) invokes it as `<notebook>:move` (the
# notebook-prefixed subcommand itself) instead, which sidesteps the bug —
# see CLAUDE.md's project-subsystem section. This script still reproduces
# the underlying `nb` quirk in isolation (useful if a future `nb` upgrade
# needs re-checking, or if the workaround itself needs revisiting), without
# going through the full Rust test harness (db init, project::create_project,
# etc.), so it's fast to iterate on and safe to re-run against different `nb`
# versions/configs.
#
# Usage: scripts/debug/nb_move_repro.sh
# Cleans up its own scratch notebooks (zz_repro_src, zz_repro_archive) on exit,
# whether it reproduces the bug or not. Uses your real `nb` install/notebooks
# directory (nb notebooks are global, not scoped by cwd) — only touches the
# two zz_repro_* notebooks it creates.

set -uo pipefail

SRC_NB="zz_repro_src"
DEST_NB="zz_repro_archive"

cleanup() {
  nb notebooks delete "$SRC_NB" --force >/dev/null 2>&1
  nb notebooks delete "$DEST_NB" --force >/dev/null 2>&1
}
trap cleanup EXIT

cleanup # in case a previous run left these behind

echo "== setup =="
nb notebooks add "$SRC_NB" >/dev/null
echo "repro body" | nb --no-color "$SRC_NB:add" --title "repro note" --content - >/dev/null
nb notebooks add "$DEST_NB" >/dev/null

echo "== nb move (this is what notes::archive_note / nb_client::nb_move does) =="
# Mirrors `nb_move(src_notebook, nb_id, &format!("{}:{}", ARCHIVE_NOTEBOOK, dest_path))`
# in notes/src/nb_client.rs, with dest_path = "<slug>/<title>".
set -x
nb --no-color move "$SRC_NB:1" "$DEST_NB:zz_repro_src/repro note" --force
move_status=$?
set +x

echo
if [ "$move_status" -eq 0 ]; then
  echo "move SUCCEEDED — the nb CLI bug may be fixed in this nb version / environment."
  nb --no-color "$DEST_NB:list" "zz_repro_src/"
else
  echo "move FAILED (exit $move_status) — reproduces the nb CLI bug this app works"
  echo "around by invoking move as '<notebook>:move' instead (see CLAUDE.md's"
  echo "project-subsystem section, and notes/src/nb_client.rs)."
fi
