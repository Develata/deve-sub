#!/bin/sh
# Deve Sub — Linux install script (DEPLOY-002).
#
# Downloads a release binary, installs it to /usr/local/bin, creates a
# dedicated system user, runs migrations, and starts a hardened systemd
# service. The script is idempotent: re-running it upgrades the binary,
# re-migrates, and repairs the service unit.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Develata/deve-sub/main/scripts/install.sh | sudo sh
#
# Environment variables:
#   DEVE_SUB_VERSION  — specific version to install (default: latest)
#   DEVE_SUB_BIND     — bind address (default: 0.0.0.0:8080)
#   DEVE_SUB_DATA_DIR — data directory (default: /var/lib/deve-sub)
#
# WARNING (DS-AUD-036): the checksum verified by this script is downloaded
# from the same unsigned GitHub Release as the binary. It detects transport
# corruption but does NOT authenticate the publisher. Until Sigstore/cosign
# signature verification is added (Phase F), treat this install path as
# convenience-only, not supply-chain secure.

set -eu

BIN_PATH="/usr/local/bin/deve-sub"
DATA_DIR="${DEVE_SUB_DATA_DIR:-/var/lib/deve-sub}"
BIND_ADDR="${DEVE_SUB_BIND:-0.0.0.0:8080}"
SERVICE_FILE="/etc/systemd/system/deve-sub.service"
REPO="Develata/deve-sub"
DEVE_USER="deve-sub"
DEVE_GROUP="deve-sub"

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

# WHY (DS-AUD-B02): the default install exposes plain HTTP on 0.0.0.0:8080.
# The config default cookie_secure=false pairs with this so browser login
# works, but the traffic is unencrypted and the session cookie is sent in
# the clear. For production, put deve-sub behind a reverse proxy (Caddy,
# nginx) terminating TLS and set cookie_secure=true + trust_proxy_headers=true
# in the config file. Print this warning so the operator is not surprised.
if [ "${BIND_ADDR%%:*}" != "127.0.0.1" ] && [ "${BIND_ADDR%%:*}" != "localhost" ]; then
    info "WARNING: serving plain HTTP on $BIND_ADDR (cookie_secure=false)."
    info "  For production, use a reverse proxy with TLS and set"
    info "  cookie_secure=true + trust_proxy_headers=true in the config file."
fi

if [ "$VERSION" = "latest" ]; then
    info "WARNING: installing 'latest' — pin DEVE_SUB_VERSION for reproducible installs"
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

if [ "$(id -u)" -ne 0 ]; then
    err "root privileges required (run with sudo or pipe to sudo sh)"
fi

# WHY (P0-03): stop the running service BEFORE overwriting the binary so
# the old process is not left with a swapped-out executable (ETXTBSY on
# Linux prevents overwriting a running binary's file, but `install` may
# succeed by unlinking+creating — leaving the old process with a deleted
# inode and the new file orphaned until restart). Stopping first is the
# safe upgrade ordering. `|| true` because the service may not exist yet
# (first install).
systemctl stop deve-sub 2>/dev/null || true

# WHY (P0-03): back up the existing binary so we can roll back if a later
# step (key init, migrate, service start) fails. First install has no
# prior binary to back up.
BINARY_BACKUP=""
if [ -f "$BIN_PATH" ]; then
    BINARY_BACKUP="$TMPDIR/deve-sub.bak"
    cp -a "$BIN_PATH" "$BINARY_BACKUP"
fi

# WHY (P0-03): trap fires on ANY exit. If the exit code is non-zero (set -e
# failure, signal, or explicit err), roll back the binary before cleaning
# up the temp dir. On exit 0 (success), only clean up. This ensures a
# failed key init / migrate / service start restores the previous binary.
rollback_binary() {
    if [ -n "$BINARY_BACKUP" ] && [ -f "$BINARY_BACKUP" ]; then
        info "rolling back to previous binary..."
        install -m 0755 "$BINARY_BACKUP" "$BIN_PATH"
    fi
}
trap 'rc=$?; if [ "$rc" -ne 0 ]; then rollback_binary; fi; rm -rf "$TMPDIR"' EXIT

info "installing binary to $BIN_PATH..."
install -m 0755 "$TMPDIR/$ASSET" "$BIN_PATH"

# Record the installed version for post-restart verification (DS-AUD-006).
INSTALLED_VERSION=$("$BIN_PATH" --version 2>/dev/null | awk '{print $NF}' || echo "unknown")
info "  installed version: $INSTALLED_VERSION"

info "creating dedicated system user/group..."
if ! getent group "$DEVE_GROUP" >/dev/null 2>&1; then
    groupadd --system "$DEVE_GROUP"
fi
if ! id -u "$DEVE_USER" >/dev/null 2>&1; then
    useradd --system --gid "$DEVE_GROUP" --shell /usr/sbin/nologin \
        --home-dir "$DATA_DIR" --no-create-home "$DEVE_USER"
fi

info "creating data directory with 0700 permissions..."
mkdir -p "$DATA_DIR"
# WHY: 0700 — the data dir contains the SQLite DB and master key; only the
# service user should have access (DS-AUD-006).
chmod 0700 "$DATA_DIR"
chown -R "$DEVE_USER:$DEVE_GROUP" "$DATA_DIR"

info "initializing master key..."
# WHY (DS-AUD-B01): serve uses MasterKey::load (strict, fail-closed) per
# ADR-0007 §7 — allow_master_key_generation defaults to false to prevent
# silent key rotation when a production mount is lost. The key must exist
# before serve starts. `key init` is the explicit bootstrap:
# - refuses if the key already exists (no accidental rotation);
# - refuses if the DB exists without a key (no silent invalidation of
#   encrypted columns);
# - creates 32 bytes from OsRng, mode 0600, fsync'd for crash durability.
# The key path is $DATA_DIR/master.key (not the config default
# data/master.key — systemd WorkingDirectory=$DATA_DIR would resolve
# that relative default to $DATA_DIR/data/master.key, an extra segment
# the operator cannot guess).
#
# WHY (P0-03): only run `key init` on FIRST install (key file absent).
# On upgrade the key already exists and `key init` would bail, breaking
# idempotency. Migrations and service restart handle the upgrade path.
if [ ! -f "$DATA_DIR/master.key" ]; then
    sudo -u "$DEVE_USER" "$BIN_PATH" key init \
        --key-path "$DATA_DIR/master.key" \
        --db-path "$DATA_DIR/deve-sub.db"
else
    info "  master key already exists — skipping key init"
fi

info "running database migrations..."
# WHY: migrate before writing the service unit and starting, so a migration
# failure aborts the install without leaving a broken service (DS-AUD-006).
# Run as the service user so the DB file is owned correctly.
sudo -u "$DEVE_USER" "$BIN_PATH" migrate --db-path "$DATA_DIR/deve-sub.db"

info "writing systemd service unit..."
cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=Deve Sub — Proxy Subscription Manager
After=network.target

[Service]
Type=simple
User=$DEVE_USER
Group=$DEVE_GROUP
ExecStart=$BIN_PATH serve --db-path $DATA_DIR/deve-sub.db --key-path $DATA_DIR/master.key --bind $BIND_ADDR
WorkingDirectory=$DATA_DIR
Restart=on-failure
RestartSec=5

# Hardening (DS-AUD-006)
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ReadWritePaths=$DATA_DIR
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
LockPersonality=true
MemoryDenyWriteExecute=true
RestrictRealtime=true
RestrictSUIDSGID=true

[Install]
WantedBy=multi-user.target
EOF

info "enabling and starting service..."
systemctl daemon-reload
# WHY: `restart` ensures an already-running instance is replaced by the new
# binary; `enable --now` alone does not guarantee a restart on upgrade.
systemctl restart deve-sub
systemctl enable deve-sub

info "waiting for healthy state..."
for i in $(seq 1 60); do
    if curl -sf "http://127.0.0.1:${BIND_ADDR##*:}/health/live" >/dev/null 2>&1; then
        info "service is healthy"
        # Verify the running binary matches the installed version (DS-AUD-006).
        RUNNING_VERSION=$(curl -sf "http://127.0.0.1:${BIND_ADDR##*:}/health/live" 2>/dev/null | grep -o '"version":"[^"]*"' | head -1 | cut -d'"' -f4 || echo "")
        if [ -n "$RUNNING_VERSION" ] && [ "$RUNNING_VERSION" != "$INSTALLED_VERSION" ]; then
            err "version mismatch: installed $INSTALLED_VERSION but service reports $RUNNING_VERSION — restart may have failed"
        fi
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
