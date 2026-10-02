#!/usr/bin/env bash
# Build manage_dan and install it as a native systemd service + nginx
# reverse proxy on this machine. Run from the project root: ./deploy.sh
#
# ── One-time setup (still manual — package names/steps vary too much to
#    safely automate) ─────────────────────────────────────────────────────────
#   sudo usermod -aG plugdev "$USER"    # USB printer access
#   sudo cp 99-printer.rules /etc/udev/rules.d/ && sudo udevadm control --reload-rules
#
# nginx itself IS auto-installed below (detects apt-get/pacman/dnf) — a fresh
# machine with no nginx package at all previously failed confusingly deep
# inside `sudo tee /etc/nginx/conf.d/manage_dan.conf` ("No such file or
# directory", since neither nginx nor its conf.d existed yet).
#
# The build toolchain (C compiler/linker, pkg-config) and the native
# libraries the Rust build links against (libudev, openssl, libusb) plus
# zip/unzip (project archiving) ARE also auto-installed, same detection.
#
# The app's runtime CLIs ARE also auto-installed: git + curl via the package
# manager; hledger (finances) as its official release binary when missing or
# older than HLEDGER_MIN, since distro packages are often too old to parse the
# app's journal; and nb (todos/notes/log) plus its `daily` plugin via nb's own
# download, since nb isn't in most distro repos.
#
# Rust/cargo IS also auto-installed below (via rustup, into the invoking
# user's ~/.cargo). rustup's installer only adds cargo to PATH for *future*
# login shells, so this script sources ~/.cargo/env itself rather than
# requiring a new shell before the build step can find `cargo`.
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$PROJECT_DIR/target/release/app"
RUN_USER="$(whoami)"

# ── Ensure nginx is installed ─────────────────────────────────────────────────
if ! command -v nginx &> /dev/null; then
  echo "nginx not found — installing..."
  if command -v pacman &> /dev/null; then
    sudo pacman -Sy --needed --noconfirm nginx
  elif command -v apt-get &> /dev/null; then
    sudo apt-get update && sudo apt-get install -y nginx
  elif command -v dnf &> /dev/null; then
    sudo dnf install -y nginx
  else
    echo "Unrecognized package manager — install nginx manually, then re-run this script." >&2
    exit 1
  fi
fi
# Defensive even after a fresh install: some distros' base nginx package
# doesn't ship an empty conf.d/ (or it was previously removed by hand).
sudo mkdir -p /etc/nginx/conf.d

# ── Ensure build toolchain + native libraries are installed ───────────────────
# cargo needs a C linker (`cc`) to link anything at all, and several crates
# (libudev-sys, openssl-sys, libusb1-sys via escpos's native_usb) build
# against system libraries found via pkg-config. rustup installs none of
# this, so a fresh machine fails at link time with "linker `cc` not found".
# Checked first (rather than always reinstalling) so routine redeploys don't
# hit the package manager every run.
if ! command -v cc &> /dev/null \
  || ! command -v pkg-config &> /dev/null \
  || ! pkg-config --exists libudev openssl libusb-1.0 \
  || ! command -v zip &> /dev/null \
  || ! command -v unzip &> /dev/null; then
  echo "Build dependencies missing — installing..."
  if command -v pacman &> /dev/null; then
    sudo pacman -Sy --needed --noconfirm base-devel pkgconf systemd-libs openssl libusb zip unzip
  elif command -v apt-get &> /dev/null; then
    sudo apt-get update && sudo apt-get install -y \
      build-essential pkg-config libudev-dev libssl-dev libusb-1.0-0-dev zip unzip
  elif command -v dnf &> /dev/null; then
    sudo dnf install -y gcc make pkgconf-pkg-config systemd-devel openssl-devel libusb1-devel zip unzip
  else
    echo "Unrecognized package manager — install a C compiler (cc), pkg-config, and the" >&2
    echo "libudev / openssl / libusb-1.0 development packages manually, then re-run." >&2
    exit 1
  fi
fi

# ── Ensure runtime CLIs (git, curl) are installed ─────────────────────────────
# nb (todos/notes/log) requires git, and the nb and hledger install steps
# below download with curl.
if ! command -v git &> /dev/null || ! command -v curl &> /dev/null; then
  echo "git/curl missing — installing..."
  if command -v pacman &> /dev/null; then
    sudo pacman -Sy --needed --noconfirm git curl
  elif command -v apt-get &> /dev/null; then
    sudo apt-get update && sudo apt-get install -y git curl
  elif command -v dnf &> /dev/null; then
    sudo dnf install -y git curl
  else
    echo "Unrecognized package manager — install git and curl manually, then re-run." >&2
    exit 1
  fi
fi

# ── Ensure a recent-enough hledger is installed ───────────────────────────────
# The app shells out to `hledger` for finances. Distro packages (Debian/Ubuntu
# especially) can be far too old: they fail to parse journal syntax the app
# writes (e.g. `~ every 2 weeks from <date>` periodic rules), so every
# finances request errors even though the subsystem reports Go (startup only
# checks hledger exists). So rather than trusting the distro package, check the
# version and, if missing or too old, install hledger's official static release
# binary to /usr/local/bin (ahead of /usr/bin on PATH, so it wins over any
# leftover distro copy). HLEDGER_MIN is the oldest version actually verified
# against this app — lower it only after testing an older one.
HLEDGER_MIN="1.52"
HLEDGER_PIN="1.52.4"
hledger_version() {
  hledger --version 2> /dev/null | grep -oE '[0-9]+(\.[0-9]+)+' | head -1
}
hledger_ok() {
  local v
  v="$(hledger_version)"
  [ -n "$v" ] && [ "$(printf '%s\n%s\n' "$HLEDGER_MIN" "$v" | sort -V | head -1)" = "$HLEDGER_MIN" ]
}
if ! hledger_ok; then
  echo "hledger $(hledger_version || true) missing or older than $HLEDGER_MIN — installing $HLEDGER_PIN..."
  if [ "$(uname -m)" = "x86_64" ]; then
    HLEDGER_TMP="$(mktemp -d)"
    curl -fsSL --connect-timeout 15 --max-time 300 \
      "https://github.com/hledgerorg/hledger/releases/download/$HLEDGER_PIN/hledger-linux-x64.tar.gz" \
      | tar xz -C "$HLEDGER_TMP" hledger
    sudo install -m 755 "$HLEDGER_TMP/hledger" /usr/local/bin/hledger
    rm -rf "$HLEDGER_TMP"
    hash -r
  else
    # hledger publishes no Linux build for other architectures (e.g. arm64).
    echo "No official hledger binary for $(uname -m). Install hledger >= $HLEDGER_MIN" >&2
    echo "yourself (e.g. a newer distro package, or build via stack/cabal), then re-run." >&2
    exit 1
  fi
  if ! hledger_ok; then
    echo "hledger on PATH is still $(hledger_version || echo missing) ($(command -v hledger || true))," >&2
    echo "expected >= $HLEDGER_MIN. Remove the old copy (e.g. sudo apt-get remove hledger), then re-run." >&2
    exit 1
  fi
fi

# ── Ensure nb + its `daily` plugin are installed ──────────────────────────────
# nb isn't packaged by most distros' main repos, so it's installed the way its
# own README describes: the single `nb` script downloaded onto PATH.
if ! command -v nb &> /dev/null; then
  echo "nb not found — installing to /usr/local/bin/nb..."
  sudo curl -fsSL --connect-timeout 15 --max-time 120 \
    https://raw.githubusercontent.com/xwmx/nb/master/nb -o /usr/local/bin/nb
  sudo chmod +x /usr/local/bin/nb
fi
# nb refuses to run until git has a global user.name/user.email: on first use
# it prompts for both in an endless `while true; read` loop. With output sent
# to /dev/null (as below) that prompt is invisible and deploy just hangs — and
# the systemd unit runs nb as this same user, so the app's own nb calls would
# stall the same way. Set an identity up front (asked for if this is an
# interactive terminal, otherwise a user@host default) so nb never prompts.
if [ -z "$(git config --global user.name || true)" ]; then
  GIT_NAME="$RUN_USER"
  if [ -t 0 ]; then
    read -r -p "git user.name for nb's commits [$GIT_NAME]: " reply
    GIT_NAME="${reply:-$GIT_NAME}"
  fi
  git config --global user.name "$GIT_NAME"
fi
if [ -z "$(git config --global user.email || true)" ]; then
  GIT_EMAIL="$RUN_USER@${HOSTNAME:-localhost}"
  if [ -t 0 ]; then
    read -r -p "git user.email for nb's commits [$GIT_EMAIL]: " reply
    GIT_EMAIL="${reply:-$GIT_EMAIL}"
  fi
  git config --global user.email "$GIT_EMAIL"
fi
# Plugins live under the invoking user's own nb dir (~/.nb/.plugins), which is
# the same user the systemd unit runs as (RUN_USER) — so this must NOT run via
# sudo. The Log feature (`nb log:daily`) fails without this plugin.
if ! nb plugins daily < /dev/null &> /dev/null; then
  echo "nb daily plugin not found — installing..."
  nb plugins install https://raw.githubusercontent.com/xwmx/nb/master/plugins/daily.nb-plugin --force < /dev/null
fi

# ── Ensure cargo is available ─────────────────────────────────────────────────
# A non-login shell (e.g. ssh "cmd", or the same shell rustup was just
# installed from) may not have ~/.cargo/bin on PATH yet even when Rust is
# installed, so try sourcing rustup's env file before deciding it's missing.
CARGO_ENV="${CARGO_HOME:-$HOME/.cargo}/env"
if ! command -v cargo &> /dev/null && [ -f "$CARGO_ENV" ]; then
  # shellcheck source=/dev/null
  . "$CARGO_ENV"
fi
if ! command -v cargo &> /dev/null; then
  echo "cargo not found — installing Rust via rustup..."
  if ! command -v curl &> /dev/null; then
    echo "curl is required to install Rust — install curl, then re-run this script." >&2
    exit 1
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  # shellcheck source=/dev/null
  . "$CARGO_ENV"
fi

# ── Build ──────────────────────────────────────────────────────────────────────
echo "Building release binary..."
cargo build --release -p app

# ── Install binary ───────────────────────────────────────────────────────────
echo "Installing binary..."
mkdir -p "$PROJECT_DIR/data/logs"
sudo install -m 755 "$BINARY" /usr/local/bin/manage_dan

# ── Install systemd service (idempotent) ─────────────────────────────────────
echo "Installing systemd service..."
sudo tee /etc/systemd/system/manage_dan.service > /dev/null << EOF
[Unit]
Description=manage_dan app
After=network.target

[Service]
ExecStart=/usr/local/bin/manage_dan
WorkingDirectory=$PROJECT_DIR
User=$RUN_USER
SupplementaryGroups=plugdev
Environment=LOG_STDOUT=true
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# ── Deploy frontend (static file + nginx config) ──────────────────────────────
echo "Deploying frontend..."
sudo mkdir -p /var/www/manage_dan
sudo install -m 644 "$PROJECT_DIR/frontend/index.html" /var/www/manage_dan/index.html

# Keep the Android app's build-time offline-fallback snapshot in sync too —
# see deploy-frontend.sh's matching step for why.
ANDROID_ASSET_DIR="$PROJECT_DIR/android/app/src/main/assets"
mkdir -p "$ANDROID_ASSET_DIR"
cp "$PROJECT_DIR/frontend/index.html" "$ANDROID_ASSET_DIR/bundled_shell.html"

sudo tee /etc/nginx/conf.d/manage_dan.conf > /dev/null << 'EOF'
server {
    listen 80;
    server_name _;

    root /var/www/manage_dan;
    index index.html;

    location /api/ {
        proxy_pass         http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_read_timeout 30s;
    }

    location /todo/ {
        proxy_pass         http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_read_timeout 30s;
    }

    location /notes/ {
        proxy_pass         http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_read_timeout 30s;
    }

    location /list/ {
        proxy_pass         http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_read_timeout 30s;
    }

    location / {
        try_files $uri $uri/ /index.html;
    }
}
EOF

# ── Restart services ──────────────────────────────────────────────────────────
echo "Restarting services..."
sudo systemctl daemon-reload
sudo systemctl enable manage_dan
sudo systemctl restart manage_dan
sudo rm -f /etc/nginx/sites-enabled/default
sudo nginx -t
sudo systemctl enable nginx
sudo systemctl reload nginx || sudo systemctl start nginx

# ── Point the hledger CLI at the app's journal ────────────────────────────────
# A bare `hledger ...` (no -f, no LEDGER_FILE) reads ~/.hledger.journal, never
# the app's own journal, so terminal queries silently showed a different (or
# empty) ledger. Symlinking it makes the CLI and the app share one file. Uses
# the default `[finances] journal_path` — if that's overridden in config, point
# LEDGER_FILE at the real path instead. Never replaces an existing file or a
# symlink pointing elsewhere.
JOURNAL="$PROJECT_DIR/data/finances.journal"
HLEDGER_DEFAULT="$HOME/.hledger.journal"
if [ ! -e "$HLEDGER_DEFAULT" ] && [ ! -L "$HLEDGER_DEFAULT" ]; then
  ln -s "$JOURNAL" "$HLEDGER_DEFAULT"
  echo "Linked $HLEDGER_DEFAULT -> $JOURNAL"
elif [ "$(readlink -f "$HLEDGER_DEFAULT")" != "$(readlink -f "$JOURNAL")" ]; then
  echo "Note: $HLEDGER_DEFAULT already exists and isn't the app's journal — left" >&2
  echo "      untouched. The hledger CLI will read it, not $JOURNAL." >&2
fi

echo "Done. App running natively on this machine."
