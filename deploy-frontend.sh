#!/usr/bin/env bash
# Deploy only frontend/index.html to the nginx-served static path. No build,
# no systemd restart — use this for frontend-only changes so the backend
# service (and its in-memory/monitor state) stays untouched.
# Run from the project root: ./deploy-frontend.sh
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Deploying frontend..."
sudo install -m 644 "$PROJECT_DIR/frontend/index.html" /var/www/manage_dan/index.html

echo "Done."
