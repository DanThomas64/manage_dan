#!/usr/bin/env bash
# Cross-compile app for linux/arm64 and deploy to homepi via systemd + nginx.
# Run from the project root: ./deploy.sh
#
# One-time Pi setup required:
#   sudo apt-get install -y nginx libudev1 libssl3
#   sudo systemctl enable nginx
#   sudo usermod -aG plugdev dan   # USB device access
set -euo pipefail

REMOTE_HOST="10.0.0.221"
REMOTE_DIR="/home/dan/manage_dan"
BUILDER="arm-builder"
DIST_DIR="$(mktemp -d)"

trap 'rm -rf "$DIST_DIR"' EXIT

# ── Ensure ARM buildx builder exists ─────────────────────────────────────────
if ! docker buildx inspect "$BUILDER" &>/dev/null; then
    echo "Creating buildx builder '$BUILDER'..."
    docker buildx create --name "$BUILDER" --driver docker-container --bootstrap
fi
docker buildx use "$BUILDER"

# ── Cross-compile and extract binary ─────────────────────────────────────────
# Reuses the existing Dockerfile builder stage; extracts the binary from the
# final image filesystem instead of loading the full image into Docker on Pi.
echo "Building app binary for linux/arm64..."
docker buildx build \
    --platform linux/arm64 \
    --output "type=local,dest=$DIST_DIR" \
    .
# Binary lands at $DIST_DIR/usr/local/bin/manage (from COPY in Dockerfile)

# ── Deploy binary ─────────────────────────────────────────────────────────────
echo "Deploying binary..."
ssh "$REMOTE_HOST" "mkdir -p $REMOTE_DIR/config $REMOTE_DIR/data/logs"
scp "$DIST_DIR/usr/local/bin/manage" "$REMOTE_HOST:/tmp/manage_dan_new"
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
