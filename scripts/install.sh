#!/bin/sh
# Deve Sub — Linux install script (DEPLOY-002).
#
# Downloads a release binary and its Web assets, installs them, creates a
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
# alongside the binary, without verifying the release signature. It detects
# corruption but does NOT authenticate the publisher. Until Sigstore/cosign
# signature verification is added (Phase F), treat this install path as
# convenience-only, not supply-chain secure.

set -eu

BIN_PATH="/usr/local/bin/deve-sub"
WEB_DIR="/usr/local/share/deve-sub/web"
WEB_ASSET="deve-sub-web.tar.gz"
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
need tar

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
    # Resolve once: independent /latest/download requests can cross a release.
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p')
fi
printf '%s\n' "$VERSION" | grep -qE '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$' \
    || err "invalid release version: $VERSION"
DOWNLOAD_BASE="https://github.com/$REPO/releases/download/$VERSION"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "downloading $ASSET..."
curl -fsSL -o "$TMPDIR/$ASSET" "$DOWNLOAD_BASE/$ASSET"
info "downloading matching frontend..."
curl -fsSL -o "$TMPDIR/$WEB_ASSET" "$DOWNLOAD_BASE/$WEB_ASSET"

info "downloading checksums..."
curl -fsSL -o "$TMPDIR/checksums.txt" "$DOWNLOAD_BASE/checksums.txt"

info "verifying checksum..."
for asset in "$ASSET" "$WEB_ASSET"; do
    EXPECTED=$(awk -v name="$asset" '$2 == name {print $1}' "$TMPDIR/checksums.txt")
    [ -n "$EXPECTED" ] || err "checksum for $asset not found in checksums.txt"
    ACTUAL=$(sha256sum "$TMPDIR/$asset" | awk '{print $1}')
    [ "$ACTUAL" = "$EXPECTED" ] || err "checksum mismatch for $asset"
done

chmod 0755 "$TMPDIR/$ASSET"
INSTALLED_VERSION=$("$TMPDIR/$ASSET" --version | awk '{print $NF}')
[ "$INSTALLED_VERSION" = "${VERSION#v}" ] || err "binary version does not match release $VERSION"

# Extract only regular files/directories into fresh staging. Links and parent
# traversal could redirect a privileged install outside its owned directory.
tar -tzf "$TMPDIR/$WEB_ASSET" > "$TMPDIR/web-entries"
while IFS= read -r entry; do
    case "$entry" in
        /*|..|../*|*/../*|*/..) err "unsafe frontend archive path" ;;
    esac
done < "$TMPDIR/web-entries"
tar -tvzf "$TMPDIR/$WEB_ASSET" > "$TMPDIR/web-types"
awk 'substr($1,1,1) != "-" && substr($1,1,1) != "d" {exit 1}' "$TMPDIR/web-types" \
    || err "frontend archive contains a link or special file"
mkdir "$TMPDIR/web"
tar --no-same-owner --no-same-permissions -xzf "$TMPDIR/$WEB_ASSET" -C "$TMPDIR/web"
[ -s "$TMPDIR/web/index.html" ] || err "frontend archive is missing index.html"
for extension in wasm js css; do
    find "$TMPDIR/web/assets" -type f -name "*.$extension" -size +0c -print -quit \
        | grep -q . || err "frontend archive is missing $extension assets"
done

if [ "$(id -u)" -ne 0 ]; then
    err "root privileges required (run with sudo or pipe to sudo sh)"
fi

# Backups and the recovery trap precede stopping the existing service.
BINARY_BACKUP=""
WEB_BACKUP=""
SERVICE_BACKUP=""
BINARY_INSTALLED=0
WEB_INSTALLED=0
UNIT_INSTALLED=0
SERVICE_STARTED=0
WAS_ACTIVE=0
if systemctl is-active --quiet deve-sub; then WAS_ACTIVE=1; fi
if [ -f "$BIN_PATH" ]; then
    BINARY_BACKUP="$TMPDIR/deve-sub.bak"
    cp -a "$BIN_PATH" "$BINARY_BACKUP"
fi
[ ! -L "$WEB_DIR" ] || err "frontend destination must not be a symlink"
if [ -d "$WEB_DIR" ]; then
    WEB_BACKUP="$TMPDIR/web.bak"
    cp -a "$WEB_DIR" "$WEB_BACKUP"
fi
if [ -f "$SERVICE_FILE" ]; then
    SERVICE_BACKUP="$TMPDIR/service.bak"
    cp -a "$SERVICE_FILE" "$SERVICE_BACKUP"
fi

# Stop the new process before restoring its assets, then restore the previous
# unit and running state. Failed recovery keeps backups for operator repair.
rollback_install() {
    if [ "$SERVICE_STARTED" -eq 1 ]; then
        systemctl stop deve-sub || return 1
    fi
    if [ "$BINARY_INSTALLED" -eq 1 ]; then
        if [ -n "$BINARY_BACKUP" ]; then
            info "rolling back to previous binary..."
            install -m 0755 "$BINARY_BACKUP" "$BIN_PATH" || return 1
        else
            rm -f "$BIN_PATH" || return 1
        fi
    fi
    if [ "$WEB_INSTALLED" -eq 1 ]; then
        rm -rf "$WEB_DIR" || return 1
        if [ -n "$WEB_BACKUP" ]; then
            cp -a "$WEB_BACKUP" "$WEB_DIR" || return 1
        fi
    fi
    if [ "$UNIT_INSTALLED" -eq 1 ]; then
        if [ -n "$SERVICE_BACKUP" ]; then
            cp -a "$SERVICE_BACKUP" "$SERVICE_FILE" || return 1
        else
            rm -f "$SERVICE_FILE" || return 1
        fi
        systemctl daemon-reload || return 1
    fi
    if [ "$WAS_ACTIVE" -eq 1 ]; then systemctl start deve-sub || return 1; fi
}
finish() {
    rc=$?
    trap - EXIT
    if [ "$rc" -ne 0 ] && ! rollback_install; then
        echo "ERROR: rollback incomplete; recovery backups retained at $TMPDIR" >&2
        exit "$rc"
    fi
    rm -rf "$TMPDIR"
    exit "$rc"
}
trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [ "$WAS_ACTIVE" -eq 1 ]; then systemctl stop deve-sub; fi

info "installing binary to $BIN_PATH..."
BINARY_INSTALLED=1
install -m 0755 "$TMPDIR/$ASSET" "$BIN_PATH"
info "installing frontend to $WEB_DIR..."
install -d -m 0755 "$(dirname "$WEB_DIR")"
WEB_INSTALLED=1
rm -rf "$WEB_DIR"
cp -a "$TMPDIR/web" "$WEB_DIR"
chmod -R u=rwX,go=rX "$WEB_DIR"

# The staged binary version was checked before any installation mutation.
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
UNIT_INSTALLED=1
cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=Deve Sub — Proxy Subscription Manager
After=network.target

[Service]
Type=simple
User=$DEVE_USER
Group=$DEVE_GROUP
ExecStart=$BIN_PATH serve --db-path $DATA_DIR/deve-sub.db --key-path $DATA_DIR/master.key --bind $BIND_ADDR --web-dist-dir $WEB_DIR
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
SERVICE_STARTED=1
systemctl restart deve-sub

info "waiting for healthy state..."
for i in $(seq 1 60); do
    if curl -sf "http://127.0.0.1:${BIND_ADDR##*:}/health/ready" >/dev/null 2>&1; then
        info "service is healthy"
        # Verify the running binary matches the installed version (DS-AUD-006).
        RUNNING_VERSION=$(curl -sf "http://127.0.0.1:${BIND_ADDR##*:}/health/live" 2>/dev/null | grep -o '"version":"[^"]*"' | head -1 | cut -d'"' -f4 || echo "")
        if [ "$RUNNING_VERSION" != "$INSTALLED_VERSION" ]; then
            err "version mismatch: installed $INSTALLED_VERSION but service reports $RUNNING_VERSION — restart may have failed"
        fi
        systemctl enable deve-sub
        echo
        echo "Deve Sub installed successfully."
        echo "  binary:  $BIN_PATH"
        echo "  web:     $WEB_DIR"
        echo "  data:    $DATA_DIR"
        echo "  service: systemctl status deve-sub"
        echo "  logs:    journalctl -u deve-sub -f"
        exit 0
    fi
    sleep 1
done

err "service did not become healthy within 60s. Check: journalctl -u deve-sub"
