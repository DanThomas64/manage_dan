#!/usr/bin/env bash
# Build manage_dan and install it as a native systemd service + nginx
# reverse proxy on this machine. Run from the project root: ./deploy.sh
#
# ── One-time setup ────────────────────────────────────────────────────────────
#   sudo apt-get install -y nginx libudev1 zip unzip
#   bash <(curl -fsSL https://raw.githubusercontent.com/xwmx/nb/master/nb) install
#   sudo usermod -aG plugdev "$USER"    # USB printer access
#   sudo cp 99-printer.rules /etc/udev/rules.d/ && sudo udevadm control --reload-rules
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$PROJECT_DIR/target/release/app"
RUN_USER="$(whoami)"

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
