#!/usr/bin/env bash
# Cross-compile app for linux/arm64 and deploy to homepi via systemd + nginx.
# Run from the project root: ./deploy.sh
#
# ── One-time dev machine setup ───────────────────────────────────────────────
#   rustup target add aarch64-unknown-linux-gnu.2.41
#   cargo install cargo-zigbuild
#   yay -S zig                        # or: zigup install latest
#
#   # Sync Pi sysroot (re-run if Pi system packages update)
#   mkdir -p ~/pi-sysroot
#   rsync -a 10.0.0.221:/usr/include/                  ~/pi-sysroot/usr/include/
#   rsync -a 10.0.0.221:/usr/lib/aarch64-linux-gnu/    ~/pi-sysroot/usr/lib/aarch64-linux-gnu/
#   rsync -a 10.0.0.221:/lib/aarch64-linux-gnu/        ~/pi-sysroot/lib/aarch64-linux-gnu/
#
# ── One-time Pi setup ────────────────────────────────────────────────────────
#   sudo apt-get install -y nginx libudev1
#   bash <(curl -fsSL https://raw.githubusercontent.com/xwmx/nb/master/nb) install
#   sudo systemctl enable nginx
#   sudo usermod -aG plugdev dan       # USB device access
set -euo pipefail

REMOTE_HOST="homepi.timmins"
REMOTE_DIR="/home/dan/manage_dan"
SYSROOT="$HOME/pi-sysroot"
BINARY="target/aarch64-unknown-linux-gnu/release/app"

# ── Cross-compile for linux/arm64 ────────────────────────────────────────────
echo "Cross-compiling for linux/arm64..."
PKG_CONFIG_SYSROOT_DIR="$SYSROOT" \
PKG_CONFIG_PATH="$SYSROOT/usr/lib/aarch64-linux-gnu/pkgconfig" \
PKG_CONFIG_ALLOW_CROSS=1 \
CFLAGS_aarch64_unknown_linux_gnu="-I$SYSROOT/usr/include -I$SYSROOT/usr/include/aarch64-linux-gnu" \
    cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.41 -p app

# ── Deploy binary ─────────────────────────────────────────────────────────────
echo "Deploying binary..."
ssh "$REMOTE_HOST" "mkdir -p $REMOTE_DIR/config $REMOTE_DIR/data/logs"
scp "$BINARY" "$REMOTE_HOST:/tmp/manage_dan_new"
ssh "$REMOTE_HOST" "sudo install -m 755 /tmp/manage_dan_new /usr/local/bin/manage_dan \
    && rm /tmp/manage_dan_new"

# ── Install systemd service (idempotent) ─────────────────────────────────────
echo "Installing systemd service..."
ssh "$REMOTE_HOST" "sudo tee /etc/systemd/system/manage_dan.service > /dev/null" << EOF
[Unit]
Description=manage_dan app
After=network.target

[Service]
ExecStart=/usr/local/bin/manage_dan
WorkingDirectory=$REMOTE_DIR/data
User=dan
SupplementaryGroups=plugdev
Environment=APP_CONFIG_DIR=$REMOTE_DIR/config
Environment=APP_LOGGING_FILE=$REMOTE_DIR/data/logs/app.log
Environment=LOG_STDOUT=true
Environment=TZ=America/Halifax
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# ── Deploy frontend (static file + nginx config) ──────────────────────────────
echo "Deploying frontend..."
ssh "$REMOTE_HOST" "sudo mkdir -p /var/www/manage_dan"
scp frontend/index.html "$REMOTE_HOST:/tmp/manage_dan_index.html"
ssh "$REMOTE_HOST" "sudo install -m 644 /tmp/manage_dan_index.html \
    /var/www/manage_dan/index.html && rm /tmp/manage_dan_index.html"

ssh "$REMOTE_HOST" "sudo tee /etc/nginx/conf.d/manage_dan.conf > /dev/null" << 'EOF'
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
echo "Restarting services on $REMOTE_HOST..."
ssh "$REMOTE_HOST" "sudo systemctl daemon-reload \
    && sudo systemctl enable manage_dan \
    && sudo systemctl restart manage_dan \
    && sudo rm -f /etc/nginx/sites-enabled/default \
    && sudo nginx -t \
    && sudo systemctl reload nginx"

echo "Done. App running on homepi."
