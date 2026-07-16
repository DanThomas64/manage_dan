# manage_dan

A personal management system built in Rust. Integrates with a self-hosted [Vikunja](https://vikunja.io) instance to manage todos, prints physical tickets to a USB thermal printer, maintains shopping lists, manages markdown notes, and exposes everything through an HTTP API, a terminal UI, a web frontend, and an Android app.

---

## What it does

| Feature | Description |
|---|---|
| **Todo management** | Create, edit, and complete tasks backed by Vikunja. New tasks automatically print a ticket. |
| **Physical printing** | Sends formatted receipts to a USB ESC/POS thermal printer (or renders to stdout for testing). |
| **Daily summary** | Prints an overdue/high-priority/upcoming task summary at a configurable hour each morning. |
| **End-of-day summary** | Prints tasks completed during the day at a configurable evening hour. |
| **Recurring tasks** | Configurable recurring tasks printed automatically when due. |
| **Shopping lists** | Category-based shopping lists with check-off, print support, and common-items templates. |
| **Notes** | Markdown notes managed by [nb](https://xwmx.github.io/nb/) and stored as `.md` files. Full-text search via `nb search`. Printable, editable, deletable from the web UI. |
| **Log** | Quick timestamped daily-log entries (title + tags + description) via `nb`'s `daily` plugin. Browse the last 7 days of entries in one place. |
| **Web frontend** | SPA served at `http://localhost`, split into four sections — Home, Lists, Notes, Log — each reachable from a Home landing page. |
| **Android app** | Native Android client for quick capture and list management. |
| **Terminal UI** | Full keyboard-driven TUI for todo, notes, shopping, and project management. |
| **System health** | Monitors and reports the status of every subsystem at startup and continuously. |

---

## Requirements

| Requirement | Notes |
|---|---|
| [Rust](https://rustup.rs) ≥ 1.87 | For building from source |
| A self-hosted [Vikunja](https://vikunja.io/docs/installing/) instance | For todo storage |
| [nb](https://xwmx.github.io/nb/) | For notes — the `notes` subsystem will show `Nogo` without it |
| nb's `daily` plugin | For the Log feature — install with `nb plugin install https://github.com/xwmx/nb/blob/master/plugins/daily.nb-plugin` |
| A USB ESC/POS thermal printer | Optional — the app runs fine without one in `terminal` mode |
| nginx | Only needed if deploying as a service (see [Deploy as a service](#deploy-as-a-service)) |

---

## Running from source

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Install system dependencies (Linux)

```bash
# Debian / Ubuntu
sudo apt install pkg-config libudev-dev libssl-dev

# Arch Linux
sudo pacman -S pkgconf systemd-libs openssl
```

### 3. Clone and configure

```bash
git clone https://github.com/DanThomas64/manage_dan.git
cd manage_dan
cp config/default.toml config/local.toml
# Edit config/local.toml with your Vikunja URL and API token (see above)
```

### 4. Run the backend server

```bash
cargo run -p app
```

The server starts on `http://0.0.0.0:8080`.

### 5. Run the TUI (separate terminal)

```bash
cargo run -p tui
```

The TUI reads the API URL from the `MANAGE_API_URL` environment variable, defaulting to `http://127.0.0.1:8080`.

| Scenario | Command |
|---|---|
| Local `cargo run -p app` | `cargo run -p tui` |
| Deployed via `deploy.sh` (behind nginx, port 80) | `MANAGE_API_URL=http://localhost cargo run -p tui` |

---

## Configuration

All configuration lives in `config/`. Files are layered in this order (later files win):

| File | Purpose |
|---|---|
| `config/default.toml` | Defaults — committed to the repo, do not edit |
| `config/local.toml` | Your local overrides — **gitignored**, put secrets here |

You can also override any setting with an environment variable prefixed `APP_`, e.g.:

```bash
APP_PRINTER_MODE=usb cargo run -p app
APP_VIKUNJA__API_TOKEN=your_token cargo run -p app
```

### Full config reference

```toml
environment = "development"

[printer]
# "usb" to send to a physical printer, "terminal" to render to stdout
mode = "terminal"

# USB printer vendor and product IDs (find with: lsusb)
vendor_id = 4070    # 0x0fe6
product_id = 33054  # 0x811e

# Characters per line on the physical receipt (check your printer's spec sheet)
characters_per_line = 42

# How often (seconds) to poll Vikunja for tasks to print
monitor_interval_secs = 30

# Hour of day (0–23) to print the daily summary
summary_hour = 8

# Summary detail level: "minimal", "standard", or "full"
# minimal  — overdue tasks only
# standard — overdue + high priority (≥ HIGH)
# full     — overdue + high priority + upcoming 7 days
summary_level = "full"

# Print an end-of-day completed-task summary
completed_summary_enabled = true

# Hour of day (0–23) to print the completed-task summary
completed_summary_hour = 20

[logging]
# Path to the log file (relative to working directory, or absolute)
file = "data/logs/app.log"

[vikunja]
base_url  = "http://localhost:3456"
api_token = ""
project_id = 1

```

---

## USB printer setup (Linux)

To allow the application to access the printer without running as root, install the included udev rule:

```bash
sudo cp 99-printer.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

Then add your user to the `lp` group:

```bash
sudo usermod -aG lp $USER
# Log out and back in for the group change to take effect
```

The default rule targets VID `0x0fe6` / PID `0x811e`. If your printer has different IDs, find them with `lsusb` and edit the rule file before copying it.

To enable USB printing, set `mode = "usb"` in `config/local.toml`.

---

## Deploy as a service

To run the app as an always-on native systemd service (auto-start on boot, restart on crash) fronted by nginx, rather than starting it by hand each time:

```bash
./deploy.sh
```

This builds a release binary, installs it to `/usr/local/bin/manage_dan`, installs a `manage_dan` systemd unit (`WorkingDirectory` is the project root, so it reads `config/local.toml` and writes `app.sqlite` / `data/logs/app.log` in place, same as `cargo run -p app`), and installs an nginx reverse proxy that serves `frontend/index.html` on port 80 and proxies `/api/` and `/todo/` to the app on `127.0.0.1:8080`.

One-time setup before the first run — see the comment block at the top of `deploy.sh`:

```bash
sudo apt-get install -y nginx libudev1
bash <(curl -fsSL https://raw.githubusercontent.com/xwmx/nb/master/nb) install
nb plugin install https://github.com/xwmx/nb/blob/master/plugins/daily.nb-plugin
sudo usermod -aG plugdev "$USER"    # USB printer access
```

Re-run `./deploy.sh` any time you want to rebuild and redeploy after making changes; it's idempotent.

```bash
systemctl status manage_dan     # Check service status
journalctl -u manage_dan -f     # Tail logs
sudo systemctl restart manage_dan
```

---

## TUI reference

Launch with `cargo run -p tui` (requires the backend server to be running).

Each section has its own colour theme: Dashboard (white), Tasks (blue), Notes (yellow), Lists (green), Project (magenta), Log (cyan).

### Dashboard

| Key | Action |
|---|---|
| `1` | Open Tasks screen |
| `2` | Open Notes screen |
| `3` | Open Lists screen |
| `4` | Open Project screen |
| `5` | Open Log screen |
| `R` | Refresh status |
| `Q` | Quit |

### Tasks screen

| Key | Action |
|---|---|
| `J` / `K` or `↑` `↓` | Navigate list |
| `A` | Add new task |
| `E` | Edit selected task |
| `C` | Toggle complete |
| `P` | Print ticket |
| `X` | Archive task |
| `R` | Refresh |
| `Q` | Back to dashboard |

**In the add/edit form:**

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Move between fields |
| `Enter` | Edit focused field |
| `Esc` | Cancel / close form |
| `<` / `>` | Previous / next month (calendar) |

### Notes screen

| Key | Action |
|---|---|
| `J` / `K` or `↑` `↓` | Navigate list |
| `N` | Create new note |
| `Enter` | View selected note |
| `P` | Print selected note |
| `R` | Refresh |
| `Q` | Back to dashboard |

**In the create form (Title → Folder → Tags → Content):**

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Move between fields |
| `Enter` | Advance to next field (or insert newline in Content) |
| `Ctrl+S` | Submit and create note |
| `Esc` | Cancel |

### Lists screen

| Key | Action |
|---|---|
| `Tab` | Switch focus between groups/categories and items |
| `J` / `K` or `↑` `↓` | Navigate list |
| `A` | Add group or item |
| `D` | Delete selected group or item |
| `Space` | Check / uncheck item |
| `C` | Clear all checked items |
| `P` | Print list |
| `R` | Refresh |
| `Q` / `Esc` | Back to dashboard |

### Log screen

Browses the same daily log backing `POST`/`GET /api/v1/notes/daily` — entries are shown newest first, list (left) + full entry viewer (right).

| Key | Action |
|---|---|
| `J` / `K` or `↑` `↓` | Navigate list |
| `Tab` | Cycle days-back filter (7 / 14 / 30 / 90) |
| `N` | Create new log entry |
| `R` | Refresh |
| `Q` / `Esc` | Back to dashboard |

**In the create form (Title → Tags → Content):**

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Move between fields |
| `Enter` | Advance to next field (or insert newline in Content) |
| `Ctrl+S` | Submit and create entry |
| `Esc` | Cancel |

---

## API reference

The backend exposes a REST API at `http://localhost:8080/api/v1` (or `http://localhost/api/v1` when running behind nginx via `deploy.sh`).

### Status

```
GET  /api/v1/status              System health (all subsystems + overall Go/NoGo)
GET  /api/v1/logs?limit=N        Latest N log entries (default 20)
```

### Todo

```
GET    /api/v1/todo                   List all tasks
POST   /api/v1/todo                   Create task
PUT    /api/v1/todo/:id               Update task
DELETE /api/v1/todo/:id               Delete task
PATCH  /api/v1/todo/:id/done          Set completed state  { "done": true }
POST   /api/v1/todo/:id/print         Print ticket
POST   /api/v1/todo/:id/archive       Archive task
GET    /api/v1/todo/summary           Summary statistics
```

### Notes

```
GET    /api/v1/notes                  List notes  (?notebook=work&tag=rust)
POST   /api/v1/notes                  Create note
GET    /api/v1/notes/search           Full-text search  (?q=query)
GET    /api/v1/notes/folders          List all notebooks
GET    /api/v1/notes/tags             List all tags
GET    /api/v1/notes/:id              Get single note (JSON)  (?notebook=work)
PUT    /api/v1/notes/:id              Update note  (?notebook=work)
DELETE /api/v1/notes/:id              Delete note  (?notebook=work)
POST   /api/v1/notes/:id/print        Print note  (?notebook=work)
GET    /notes/:id                     HTML viewer (markdown rendered in browser)  (?notebook=work)
```

### Log

Backed by nb's `daily` plugin, writing into a dedicated `log` notebook (excluded from the Notes endpoints above).

```
POST   /api/v1/notes/daily            Add a log entry  { "title": "...", "content": "...", "tags": [...] }
GET    /api/v1/notes/daily?days=7     List entries from the last N days (default 7), most recent first
```

### Lists

```
GET    /api/v1/lists/groups                         List all groups
POST   /api/v1/lists/groups                         Create group  { "name": "..." }
DELETE /api/v1/lists/groups/:id                     Delete group + all categories
GET    /api/v1/lists/groups/:id/categories          List categories in group
POST   /api/v1/lists/groups/:id/categories          Create category  { "name": "..." }
DELETE /api/v1/lists/categories/:id                 Delete category + all items
GET    /api/v1/lists/categories/:id/items           List items
POST   /api/v1/lists/categories/:id/items           Add item  { "name": "...", "quantity": "..." }
POST   /api/v1/lists/categories/:id/clear           Remove all checked items
POST   /api/v1/lists/categories/:id/print           Print list
GET    /api/v1/lists/categories/:id/common          List common-item templates
POST   /api/v1/lists/categories/:id/common          Add common-item template
POST   /api/v1/lists/common/:id/add                 Add common item to active list
DELETE /api/v1/lists/common/:id                     Delete common-item template
PATCH  /api/v1/lists/items/:id/check                Toggle check  { "checked": true }
DELETE /api/v1/lists/items/:id                      Delete item
```

---

## Project structure

```
manage_dan/
├── app/          Main server — HTTP API, system init, background tasks
├── db/           SQLite access layer (logs, print records, lists, notes index)
├── log/          Logging subsystem initialisation
├── printer/      ESC/POS USB printer + terminal renderer
├── todo/         Todo business logic, Vikunja integration, print monitor, summaries
├── vikunja/      Vikunja HTTP client
├── lists/        Shopping list CRUD and printing
├── notes/        Markdown notes — nb CLI backend, full-text search via nb, printing
├── project/      Project subsystem (stub)
├── tui/          Terminal UI client
├── frontend/     Web UI (vanilla JS, served by nginx) — Home, Lists, Notes, Log
├── android/      Android client app
├── config/       Configuration files
└── deploy.sh     Build + install as a native systemd service + nginx reverse proxy
```

---

## Troubleshooting

**The server starts but todos don't load**
Check that your Vikunja `base_url` and `api_token` are correct in `config/local.toml`. The system status endpoint (`GET /api/v1/status`) will show `todo: Nogo` if initialisation failed.

**USB printer not found**
Run `lsusb` to confirm the printer is detected. Check that the udev rule is installed and that your user is in the `lp` group (or `plugdev`, if running via `deploy.sh`). Verify the VID/PID in `config/local.toml` matches your printer.

**Permission denied on USB device**
The udev rule may not have been applied. Try unplugging and replugging the printer, or run `sudo udevadm trigger`. The rule sets `MODE="0666"` which allows access without root or group membership.

**TUI can't connect**
The TUI defaults to `http://127.0.0.1:8080`. If running via `deploy.sh` with nginx, set `MANAGE_API_URL=http://localhost`. Make sure the server is running before launching the TUI.

**Notes not appearing**
Notes are served live via `nb` on every request. If you edit files outside of `nb`, run `nb index reconcile` to resync the nb index.
