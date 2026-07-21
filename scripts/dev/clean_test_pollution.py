#!/usr/bin/env python3
"""Finds and removes integration-test data that leaked into the real,
live `nb` notebooks (todos, notes, and daily-log entries) instead of
staying in a test's own isolated notebook/NB_DIR.

Background: `todo/tests/nb_backend.rs`'s `nb_backend_end_to_end` test used
to isolate its todo item by notebook name only (`zz_test_todo`), not by a
full `NB_DIR` swap. Completing that todo runs `todo::log_completion`, which
always logs to the hardcoded, global `log` notebook regardless of which
todo notebook was used — a scratch *notebook name* alone doesn't isolate
that side effect. Every `cargo test` run before the fix (see that test's
own doc comment) silently wrote a real "Completed: Integration test todo"
entry into the real, live daily log. This script cleans up that specific
historical pollution, and — going forward — anything tagged/titled per the
convention this incident established: a todo/note that risks landing in a
notebook it doesn't fully control (most commonly the shared `log`
notebook, via a completion side effect) should have a title starting with
"zz_test:" or a "zz-test-data" tag, so it stays greppable/cleanable even
outside a fully-isolated test.

Usage:
    scripts/dev/clean_test_pollution.py            # dry run — report only
    scripts/dev/clean_test_pollution.py --apply     # actually remove/edit

What it checks:
  - Every `nb daily` log file (default: ~/.nb/log/*.md) for entries whose
    title matches a known test marker — the historical "Completed:
    Integration test todo" string, or the "zz_test:"-prefixed convention —
    and rewrites the file with just those entries removed (surgical: only
    the matching `## HH:MM:SS` block, not the whole day's file).
  - Every real `nb` notebook (skipping `log`, handled above, and any
    scratch notebook already named `zz_*`) for notes/todos whose title
    starts with "zz_test" or carries a "zz-test-data" tag, and deletes
    just those items.
  - Leftover `zz_*`-prefixed scratch notebooks themselves (a test that
    crashed before its own cleanup ran) — deletes the whole notebook.

Safe to re-run any time — dry run reports zero once there's nothing left
to clean.
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

TEST_TITLE_PATTERNS = [
    re.compile(r"^zz_test:", re.IGNORECASE),
    re.compile(r"^Completed: zz_test:", re.IGNORECASE),
    # Historical pollution from the pre-fix nb_backend_end_to_end test —
    # kept here so already-existing entries get cleaned even though new
    # runs no longer produce this exact title.
    re.compile(r"^Completed: Integration test todo$"),
]
TEST_TAG = "zz-test-data"

DAILY_HEADER_RE = re.compile(r"^## \d{2}:\d{2}:\d{2}$")


def run(args):
    return subprocess.run(args, capture_output=True, text=True)


def is_test_title(title: str) -> bool:
    return any(p.search(title) for p in TEST_TITLE_PATTERNS)


def entry_title(block_lines):
    for line in block_lines:
        stripped = line.strip()
        if stripped.startswith("# "):
            return stripped[2:].strip()
    return ""


def entry_tags(block_lines):
    for line in block_lines:
        stripped = line.strip()
        if stripped and all(tok.startswith("#") for tok in stripped.split()):
            return [tok[1:] for tok in stripped.split()]
    return []


def split_daily_entries(text: str):
    """Returns (header, [(time_line, body_lines), ...])."""
    lines = text.splitlines()
    header_lines = []
    i = 0
    while i < len(lines) and not DAILY_HEADER_RE.match(lines[i]):
        header_lines.append(lines[i])
        i += 1

    entries = []
    while i < len(lines):
        time_line = lines[i]
        i += 1
        body = []
        while i < len(lines) and not DAILY_HEADER_RE.match(lines[i]):
            body.append(lines[i])
            i += 1
        entries.append((time_line, body))
    return header_lines, entries


def clean_daily_logs(log_dir: Path, apply: bool):
    removed_total = 0
    for path in sorted(log_dir.glob("*.md")):
        text = path.read_text()
        header, entries = split_daily_entries(text)

        kept = []
        removed = []
        for time_line, body in entries:
            title = entry_title(body)
            tags = entry_tags(body)
            if is_test_title(title) or TEST_TAG in tags:
                removed.append((time_line.strip(), title))
            else:
                kept.append((time_line, body))

        if not removed:
            continue

        removed_total += len(removed)
        print(f"{path}:")
        for time_line, title in removed:
            print(f"  - {time_line}  {title!r}")

        if apply:
            new_lines = list(header)
            for time_line, body in kept:
                new_lines.append(time_line)
                new_lines.extend(body)
            new_text = "\n".join(new_lines).rstrip("\n") + "\n"
            path.write_text(new_text)

    return removed_total


def list_notebooks():
    out = run(["nb", "notebooks"]).stdout
    return [line.strip() for line in out.splitlines() if line.strip()]


def list_notebook_items(notebook: str):
    """Returns [(id, path)] for every item directly in `notebook`'s root —
    good enough for this cleanup since test items are never nested."""
    out = run(["nb", "--no-color", f"{notebook}:list", "--paths"]).stdout
    items = []
    for line in out.splitlines():
        line = line.strip()
        if "\U0001F4C2" in line:  # folder entry, skip
            continue
        m = re.match(r"^\[(?:[^:\]]+:)?(\d+)\]\s+(.+)$", line)
        if m:
            items.append((int(m.group(1)), m.group(2)))
    return items


def clean_notebook_items(apply: bool):
    removed_total = 0
    for notebook in list_notebooks():
        if notebook in ("log",) or notebook.startswith("zz_"):
            continue  # log handled separately; zz_ notebooks handled below
        for item_id, path in list_notebook_items(notebook):
            try:
                content = Path(path).read_text(errors="replace")
            except OSError:
                continue
            first_line = next((l for l in content.splitlines() if l.strip()), "")
            title = first_line[2:].strip() if first_line.startswith("# ") else first_line
            tags_line = next(
                (l.strip() for l in content.splitlines()[1:4]
                 if l.strip() and all(t.startswith("#") for t in l.split())),
                "",
            )
            tags = [t[1:] for t in tags_line.split()]
            if is_test_title(title) or TEST_TAG in tags:
                removed_total += 1
                print(f"{notebook}:{item_id}  {title!r}")
                if apply:
                    run(["nb", f"{notebook}:delete", str(item_id), "--force"])
    return removed_total


def clean_stray_scratch_notebooks(apply: bool):
    removed = [n for n in list_notebooks() if n.startswith("zz_")]
    for name in removed:
        print(f"stray scratch notebook: {name}")
        if apply:
            run(["nb", "notebooks", "delete", name, "--force"])
    return len(removed)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--apply", action="store_true", help="actually remove/edit — default is dry-run/report-only")
    parser.add_argument("--nb-dir", default=str(Path.home() / ".nb"), help="nb data directory (default: ~/.nb)")
    args = parser.parse_args()

    log_dir = Path(args.nb_dir) / "log"
    mode = "APPLYING changes" if args.apply else "DRY RUN (pass --apply to actually remove/edit)"
    print(f"=== {mode} ===\n")

    print("-- Daily log entries --")
    n1 = clean_daily_logs(log_dir, args.apply) if log_dir.is_dir() else 0
    if not n1:
        print("  none found")

    print("\n-- Notes/todos in real notebooks --")
    n2 = clean_notebook_items(args.apply)
    if not n2:
        print("  none found")

    print("\n-- Stray zz_* scratch notebooks --")
    n3 = clean_stray_scratch_notebooks(args.apply)
    if not n3:
        print("  none found")

    total = n1 + n2 + n3
    print(f"\n{'Removed' if args.apply else 'Would remove'}: {total} item(s) total")
    if not args.apply and total:
        print("Re-run with --apply to actually remove them.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
