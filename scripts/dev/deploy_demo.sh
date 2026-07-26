#!/usr/bin/env bash
# Serves frontend/index.html through nginx (proxying to the app already
# running on port 8080) so the demo finances data populated via the API can
# actually be viewed in a browser — the app itself doesn't serve the SPA's
# static index.html, only /api/, /todo/, /notes/, /list/ (nginx's job in the
# real deploy.sh). This is the demo-scoped equivalent of that piece: unlike
# deploy.sh, it does NOT install a systemd service (the app is expected to
# already be running via a plain `cargo run -p app`) and uses its own nginx
# config filename + a dedicated port (8090, not 80), so it never collides
# with or overwrites a real deploy.sh deployment on the same machine.
#
# Usage: scripts/dev/deploy_demo.sh
#
# nginx is auto-installed below if missing (detects apt-get/pacman/dnf) — no
# manual one-time setup needed for it specifically.
#
# Teardown:
#   sudo rm -f /etc/nginx/conf.d/manage_dan_demo.conf
#   sudo rm -rf /var/www/manage_dan_demo
#   sudo systemctl reload nginx
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEMO_PORT="${1:-8090}"
APP_PORT=8080

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

echo "Checking the app is already running on port $APP_PORT..."
if ! curl -fs "http://127.0.0.1:$APP_PORT/api/v1/status" > /dev/null; then
  echo "No app responding on http://127.0.0.1:$APP_PORT — start it first, e.g.:" >&2
  echo "  cargo run -p app" >&2
  exit 1
fi

echo "Deploying frontend static file..."
sudo mkdir -p /var/www/manage_dan_demo
sudo install -m 644 "$PROJECT_DIR/frontend/index.html" /var/www/manage_dan_demo/index.html

echo "Writing nginx demo config on port $DEMO_PORT..."
sudo tee /etc/nginx/conf.d/manage_dan_demo.conf > /dev/null << EOF
server {
    listen $DEMO_PORT;
    server_name _;

    root /var/www/manage_dan_demo;
    index index.html;

    location /api/ {
        proxy_pass         http://127.0.0.1:$APP_PORT;
        proxy_http_version 1.1;
        proxy_set_header   Host              \$host;
        proxy_set_header   X-Real-IP         \$remote_addr;
        proxy_set_header   X-Forwarded-For   \$proxy_add_x_forwarded_for;
        proxy_read_timeout 30s;
    }

    location /todo/ {
        proxy_pass         http://127.0.0.1:$APP_PORT;
        proxy_http_version 1.1;
        proxy_set_header   Host              \$host;
        proxy_set_header   X-Real-IP         \$remote_addr;
        proxy_set_header   X-Forwarded-For   \$proxy_add_x_forwarded_for;
        proxy_read_timeout 30s;
    }

    location /notes/ {
        proxy_pass         http://127.0.0.1:$APP_PORT;
        proxy_http_version 1.1;
        proxy_set_header   Host              \$host;
        proxy_set_header   X-Real-IP         \$remote_addr;
        proxy_set_header   X-Forwarded-For   \$proxy_add_x_forwarded_for;
        proxy_read_timeout 30s;
    }

    location /list/ {
        proxy_pass         http://127.0.0.1:$APP_PORT;
        proxy_http_version 1.1;
        proxy_set_header   Host              \$host;
        proxy_set_header   X-Real-IP         \$remote_addr;
        proxy_set_header   X-Forwarded-For   \$proxy_add_x_forwarded_for;
        proxy_read_timeout 30s;
    }

    location / {
        try_files \$uri \$uri/ /index.html;
    }
}
EOF

sudo nginx -t
sudo systemctl enable nginx > /dev/null 2>&1 || true
sudo systemctl reload nginx || sudo systemctl start nginx

echo ""
echo "Demo frontend live at: http://localhost:$DEMO_PORT"
echo "(proxying to the app already running on 127.0.0.1:$APP_PORT — leave that running)"
