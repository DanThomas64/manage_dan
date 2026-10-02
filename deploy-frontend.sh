#!/usr/bin/env bash
# Deploy only frontend/index.html to the nginx-served static path. No build,
# no systemd restart — use this for frontend-only changes so the backend
# service (and its in-memory/monitor state) stays untouched.
# Run from the project root: ./deploy-frontend.sh
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Deploying frontend..."
sudo install -m 644 "$PROJECT_DIR/frontend/index.html" /var/www/manage_dan/index.html

# Keep the Android app's build-time offline-fallback snapshot in sync too —
# a brand-new install with zero connectivity ever falls back to this bundled
# copy (see MainActivity.kt's shouldInterceptRequest) when it has no
# previously-cached live copy to use instead. Stale between runs of this
# script, same as any offline-first app's bundled-asset tradeoff — not
# rebuilt/repackaged here, just kept current on disk for the next Android build.
ANDROID_ASSET_DIR="$PROJECT_DIR/android/app/src/main/assets"
mkdir -p "$ANDROID_ASSET_DIR"
cp "$PROJECT_DIR/frontend/index.html" "$ANDROID_ASSET_DIR/bundled_shell.html"

echo "Done."
