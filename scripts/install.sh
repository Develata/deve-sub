#!/bin/sh
# Deve Sub — Linux install script (DEPLOY-002).
#
# Downloads the latest release binary, installs it to /usr/local/bin,
# creates a systemd service, and starts it. The script is idempotent:
# re-running it upgrades the binary and repairs the service unit.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Develata/deve-sub/main/scripts/install.sh | sh
#
# Environment variables:
#   DEVE_SUB_VERSION  — specific version to install (default: latest)
#   DEVE_SUB_BIND     — bind address (default: 0.0.0.0:8080)
#   DEVE_SUB_DATA_DIR — data directory (default: /var/lib/deve-sub)

set -eu

BIN_PATH="/usr/local/bin/deve-sub"
DATA_DIR="${DEVE_SUB_DATA_DIR:-/var/lib/deve-sub}"
BIND_ADDR="${DEVE_SUB_BIND:-0.0.0.0:8080}"
SERVICE_FILE="/etc/systemd/system/deve-sub.service"
REPO="Develata/deve-sub"

err() { echo "ERROR: $*" >&2; exit 1; }
info() { echo "  $*"; }

need() { command -v "$1" >/dev/null 2>&1 || err "missing dependency: $1"; }

need curl
need sha256sum
need uname
need systemctl

OS="$(uname -s)"
ARCH="$(uname -m)"

[ "$OS" = "Linux" ] || err "this script only supports Linux (got $OS)"

case "$ARCH" in
    x86_64|amd64) ASSET="deve-sub-linux-amd64" ;;
    aarch64|arm64) ASSET="deve-sub-linux-arm64" ;;
    *) err "unsupported architecture: $ARCH" ;;
esac

VERSION="${DEVE_SUB_VERSION:-latest}"

info "Deve Sub installer"
info "  architecture: $ARCH"
info "  version:      $VERSION"
info "  bind:         $BIND_ADDR"
info "  data dir:     $DATA_DIR"

if [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/$ASSET"
    CHECKSUM_URL="https://github.com/$REPO/releases/latest/download/checksums.txt"
else
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
    CHECKSUM_URL="https://github.com/$REPO/releases/download/$VERSION/checksums.txt"
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "downloading $ASSET..."
curl -fsSL -o "$TMPDIR/$ASSET" "$DOWNLOAD_URL"

info "downloading checksums..."
curl -fsSL -o "$TMPDIR/checksums.txt" "$CHECKSUM_URL"

info "verifying checksum..."
EXPECTED=$(grep "$ASSET" "$TMPDIR/checksums.txt" | awk '{print $1}')
[ -n "$EXPECTED" ] || err "checksum for $ASSET not found in checksums.txt"
ACTUAL=$(sha256sum "$TMPDIR/$ASSET" | awk '{print $1}')
[ "$ACTUAL" = "$EXPECTED" ] || err "checksum mismatch: expected $EXPECTED, got $ACTUAL"

info "installing binary to $BIN_PATH..."
if [ "$(id -u)" -ne 0 ]; then
    err "root privileges required (run with sudo or pipe to sudo sh)"
fi
install -m 0755 "$TMPDIR/$ASSET" "$BIN_PATH"

info "creating data directory..."
mkdir -p "$DATA_DIR"
chmod 0755 "$DATA_DIR"

info "writing systemd service unit..."
cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=Deve Sub — Proxy Subscription Manager
After=network.target

[Service]
Type=simple
ExecStart=$BIN_PATH serve --db-path $DATA_DIR/deve-sub.db --bind $BIND_ADDR
WorkingDirectory=$DATA_DIR
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

info "enabling and starting service..."
systemctl daemon-reload
systemctl enable --now deve-sub

info "waiting for healthy state..."
for i in $(seq 1 60); do
    if curl -sf "http://127.0.0.1:${BIND_ADDR##*:}/health/live" >/dev/null 2>&1; then
        info "service is healthy"
        echo
        echo "Deve Sub installed successfully."
        echo "  binary:  $BIN_PATH"
        echo "  data:    $DATA_DIR"
        echo "  service: systemctl status deve-sub"
        echo "  logs:    journalctl -u deve-sub -f"
        exit 0
    fi
    sleep 1
done

err "service did not become healthy within 60s. Check: journalctl -u deve-sub"
