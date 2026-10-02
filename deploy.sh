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
# The app's runtime CLIs ARE also auto-installed: hledger (finances), git +
# curl via the package manager, and nb (todos/notes/log) plus its `daily`
# plugin via nb's own download, since nb isn't in most distro repos.
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

# ── Ensure runtime CLIs (hledger, git, curl) are installed ────────────────────
# The app shells out to `hledger` for finances (subsystem goes Nogo without
# it) and to `nb` for todos/notes/log; nb itself requires git, and the nb
# install step below downloads it with curl.
if ! command -v hledger &> /dev/null \
  || ! command -v git &> /dev/null \
  || ! command -v curl &> /dev/null; then
  echo "hledger/git/curl missing — installing..."
  if command -v pacman &> /dev/null; then
    sudo pacman -Sy --needed --noconfirm hledger git curl
  elif command -v apt-get &> /dev/null; then
    sudo apt-get update && sudo apt-get install -y hledger git curl
  elif command -v dnf &> /dev/null; then
    sudo dnf install -y hledger git curl
  else
    echo "Unrecognized package manager — install hledger, git and curl manually, then re-run." >&2
    exit 1
  fi
fi

# ── Ensure nb + its `daily` plugin are installed ──────────────────────────────
# nb isn't packaged by most distros' main repos, so it's installed the way its
# own README describes: the single `nb` script downloaded onto PATH.
if ! command -v nb &> /dev/null; then
  echo "nb not found — installing to /usr/local/bin/nb..."
  sudo curl -fsSL https://raw.githubusercontent.com/xwmx/nb/master/nb -o /usr/local/bin/nb
  sudo chmod +x /usr/local/bin/nb
fi
# Plugins live under the invoking user's own nb dir (~/.nb/.plugins), which is
# the same user the systemd unit runs as (RUN_USER) — so this must NOT run via
# sudo. The Log feature (`nb log:daily`) fails without this plugin.
if ! nb plugins daily &> /dev/null; then
  echo "nb daily plugin not found — installing..."
  nb plugins install https://raw.githubusercontent.com/xwmx/nb/master/plugins/daily.nb-plugin --force
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

echo "Done. App running natively on this machine."
