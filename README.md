# manage_dan

A personal management system built in Rust. Integrates with a self-hosted [Vikunja](https://vikunja.io) instance to manage todos, prints physical tickets to a USB thermal printer, maintains shopping lists, and exposes everything through an HTTP API, a terminal UI, and a web frontend.

---

## What it does

| Feature | Description |
|---|---|
| **Todo management** | Create, edit, and complete tasks backed by Vikunja. New tasks automatically print a ticket. |
| **Physical printing** | Sends formatted receipts to a USB ESC/POS thermal printer (or renders to stdout for testing). |
| **Daily summary** | Prints an overdue/high-priority/upcoming task summary at a configurable hour each morning. |
| **Shopping lists** | Category-based shopping lists (Groceries, Toiletries, etc.) with check-off and print support. |
| **Web frontend** | Simple shopping list UI served at `http://localhost`. |
| **Terminal UI** | Full keyboard-driven TUI for todo and shopping management. |
| **System health** | Monitors and reports the status of every subsystem at startup and continuously. |

---

## Requirements

| Requirement | Notes |
|---|---|
| [Rust](https://rustup.rs) ≥ 1.87 | For building from source |
| [Docker](https://docs.docker.com/get-docker/) + [Compose](https://docs.docker.com/compose/) | For the containerised setup |
| A self-hosted [Vikunja](https://vikunja.io/docs/installing/) instance | For todo storage |
| A USB ESC/POS thermal printer | Optional — the app runs fine without one in `terminal` mode |

---

## Quick start (Docker)

This is the recommended way to run the application.

### 1. Clone the repository

```bash
git clone https://github.com/DanThomas64/manage_dan.git
cd manage_dan
```

### 2. Create your local config

Copy the example and fill in your Vikunja details:

```bash
cp config/default.toml config/local.toml
```

Edit `config/local.toml`:

```toml
[vikunja]
base_url = "https://your-vikunja-instance.example.com"
api_token = "your_api_token_here"
project_id = 1
```

> **Getting a Vikunja API token:** Log in to your Vikunja instance → Settings → API Tokens → Create a token.

You can also override any other setting from `config/default.toml` here. `config/local.toml` is gitignored and will never be committed.

### 3. Start the stack

```bash
docker compose up -d
```

The first build compiles the entire Rust workspace and will take a few minutes. Subsequent starts use the Docker layer cache and are near-instant.

| Service | URL |
|---|---|
| Shopping list web UI | http://localhost |
| Raw API | http://localhost/api/v1/... |

### 4. Stop the stack

```bash
docker compose down
```

Your data (SQLite database) is stored in a Docker named volume (`app_data`) and persists across restarts. To wipe it entirely:

```bash
docker compose down -v
```

---

## USB printing with Docker

By default the app runs in `terminal` mode (output goes to stdout). To use a physical printer with the Docker stack you need to pass the host USB bus into the container.

### 1. Install the udev rule on the host

This sets the device permissions to world-readable/writable so the container can access it without running as root.

```bash
sudo cp 99-printer.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### 2. Plug in your printer and confirm it is detected

```bash
lsusb
# Look for a line like:
# Bus 001 Device 003: ID 0fe6:811e ...
```

### 3. Enable USB passthrough in docker-compose.yml

Uncomment the `devices` block in the `app` service:

```yaml
    devices:
      - /dev/bus/usb:/dev/bus/usb
```

This mounts the entire host USB bus into the container. The app will find the printer by its VID/PID configured in `config/local.toml`.

### 4. Set printer mode to USB

In `config/local.toml`:

```toml
[printer]
mode = "usb"
```

Or via environment variable without editing the file:

```bash
APP_PRINTER_MODE=usb docker compose up -d
```

### 5. Verify the VID and PID match your printer

The defaults are `0x0fe6` / `0x811e`. If your printer is different, find its IDs with `lsusb` and override them in `config/local.toml`:

```toml
[printer]
mode = "usb"
vendor_id  = 0x1234   # replace with your printer's VID
product_id = 0x5678   # replace with your printer's PID
```

> **Note:** The printer must be plugged in before `docker compose up`. If you plug it in afterwards, restart the `app` container: `docker compose restart app`.

---

## Running from source

Use this approach if you want to run the TUI or develop locally.

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
| Docker Compose (via nginx, port 80) | `MANAGE_API_URL=http://localhost cargo run -p tui` |
| Docker Compose (direct, port 8080) | `MANAGE_API_URL=http://localhost:8080 cargo run -p tui` |

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

## TUI reference

Launch with `cargo run -p tui` (requires the backend server to be running).

### Dashboard

| Key | Action |
|---|---|
| `1` | Open Todo screen |
| `2` | Open Notes screen |
| `3` | Open Project screen |
| `4` | Open Shopping screen |
| `R` | Refresh status |
| `Q` | Quit |

### Todo screen

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
| `I` / `Enter` | Enter insert mode on focused field |
| `H` `J` `K` `L` / arrows | Move between fields |
| `Esc` / `Ctrl+C` | Exit insert mode |
| `<` / `>` | Previous / next month (calendar) |
| `Esc` (Normal mode) | Cancel and close form |

### Shopping screen

| Key | Action |
|---|---|
| `Tab` | Switch focus between categories and items |
| `J` / `K` or `↑` `↓` | Navigate list |
| `A` | Add category (when categories focused) / Add item (when items focused) |
| `D` | Delete selected category or item |
| `Space` | Check / uncheck item |
| `C` | Clear all checked items |
| `P` | Print list |
| `R` | Refresh |
| `Q` / `Esc` | Back to dashboard |

---

## API reference

The backend exposes a REST API at `http://localhost:8080/api/v1` (or `http://localhost/api/v1` when running through Docker).

### Status

```
GET  /api/v1/status              System health (all subsystems + overall Go/NoGo)
GET  /api/v1/logs?limit=N        Latest N log entries (default 20)
```

### Todo

```
GET    /api/v1/todo              List all tasks
POST   /api/v1/todo              Create task
PUT    /api/v1/todo/:id          Update task
DELETE /api/v1/todo/:id          Delete task
POST   /api/v1/todo/:id/print    Print ticket
POST   /api/v1/todo/:id/archive  Archive task
GET    /api/v1/todo/summary      Summary statistics
```

### Shopping

```
GET    /api/v1/shopping/categories              List categories
POST   /api/v1/shopping/categories              Create category  { "name": "..." }
DELETE /api/v1/shopping/categories/:id          Delete category + all items
GET    /api/v1/shopping/categories/:id/items    List items
POST   /api/v1/shopping/categories/:id/items    Add item  { "name": "...", "quantity": "..." }
POST   /api/v1/shopping/categories/:id/clear    Remove all checked items
POST   /api/v1/shopping/categories/:id/print    Print list
PATCH  /api/v1/shopping/items/:id/check         Toggle check  { "checked": true }
DELETE /api/v1/shopping/items/:id               Delete item
```

---

## Project structure

```
manage_dan/
├── app/          Main server — HTTP API, system init, background tasks
├── db/           SQLite access layer (logs, print records, shopping data)
├── log/          Logging subsystem initialisation
├── printer/      ESC/POS USB printer + terminal renderer
├── todo/         Todo business logic, Vikunja integration, print monitor
├── vikunja/      Vikunja HTTP client
├── shopping/     Shopping list CRUD and printing
├── notes/        Notes subsystem (stub, in progress)
├── project/      Project subsystem (stub, in progress)
├── tui/          Terminal UI client
├── frontend/     Shopping list web UI (nginx + vanilla JS)
├── config/       Configuration files
├── Dockerfile    Multi-stage Rust build
└── docker-compose.yml
```

---

## Troubleshooting

**The server starts but todos don't load**
Check that your Vikunja `base_url` and `api_token` are correct in `config/local.toml`. The system status endpoint (`GET /api/v1/status`) will show `todo: Nogo` if initialisation failed.

**USB printer not found (running from source)**
Run `lsusb` to confirm the printer is detected. Check that the udev rule is installed and that your user is in the `lp` group. Verify the VID/PID in `config/local.toml` matches your printer.

**USB printer not found (Docker)**
Make sure the `devices: - /dev/bus/usb:/dev/bus/usb` block is uncommented in `docker-compose.yml`, the udev rule is installed on the host, and `APP_PRINTER_MODE=usb` is set. The printer must be plugged in before the container starts — if you plugged it in afterwards, run `docker compose restart app`.

**Permission denied on USB device**
The udev rule may not have been applied. Try unplugging and replugging the printer, or run `sudo udevadm trigger`. The rule sets `MODE="0666"` which allows access without root or group membership.

**Docker build fails**
Ensure Docker BuildKit is enabled (`DOCKER_BUILDKIT=1`) and that you have an internet connection for downloading crates during the first build.

**TUI can't connect**
The TUI defaults to `http://127.0.0.1:8080`. If you're running via Docker Compose, set `MANAGE_API_URL=http://localhost` (nginx proxy) or `MANAGE_API_URL=http://localhost:8080` (direct, requires uncommenting the port mapping in `docker-compose.yml`). Make sure the server is running before launching the TUI.
