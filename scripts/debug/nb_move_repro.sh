#!/usr/bin/env bash
# Minimal repro for a pre-existing failure in
# `project/tests/archive_restore_delete.rs` (`archive_then_restore_then_delete`):
# after `restore_project`, the restored note count is 0 instead of 1.
#
# This isolates the one `nb` invocation that test relies on — moving a note
# across notebooks into a *new subfolder path* in one step — without going
# through the full Rust test harness (db init, project::create_project, etc.),
# so it's fast to iterate on and safe to re-run against different `nb`
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
  echo "move SUCCEEDED — bug may be fixed / environment-specific. Checking the folder listing:"
  nb --no-color "$DEST_NB:list" "zz_repro_src/"
else
  echo "move FAILED (exit $move_status) — reproduces the bug described in"
  echo "project_manage_dan_nb_move_bug memory / project/tests/archive_restore_delete.rs."
fi
