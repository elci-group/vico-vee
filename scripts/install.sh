#!/usr/bin/env bash
#
# Install the latest vico-vee release binary, create data directories, install
# the systemd unit, and generate an initial API key file.
#
# Usage: curl -sSL https://raw.githubusercontent.com/vico-systems/vico-vee/main/scripts/install.sh | sudo bash

set -euo pipefail

REPO="vico-systems/vico-vee"
INSTALL_DIR="/usr/local/bin"
DATA_DIR="/var/lib/vico-vee"
CONFIG_DIR="/etc/vico-vee"
USER_NAME="vico-vee"

ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

# Map architecture names to GitHub release asset names.
case "$ARCH" in
    x86_64)  ASSET_ARCH="x86_64" ;;
    aarch64|arm64) ASSET_ARCH="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

case "$OS" in
    linux)   ASSET_OS="linux" ;;
    darwin)  ASSET_OS="macos" ;;
    *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

ASSET_NAME="vico-vee-${ASSET_OS}-${ASSET_ARCH}.tar.gz"

# Fetch the latest release download URL.
LATEST_URL=$(curl -sSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -o '"browser_download_url": "[^"]*' \
    | grep "$ASSET_NAME" \
    | head -n 1 \
    | sed 's/"browser_download_url": "//')

if [[ -z "$LATEST_URL" ]]; then
    echo "Could not find release asset for ${ASSET_OS}/${ASSET_ARCH}" >&2
    exit 1
fi

echo "Downloading ${ASSET_NAME}..."
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT
curl -sSL "$LATEST_URL" -o "${TMP_DIR}/${ASSET_NAME}"
tar -xzf "${TMP_DIR}/${ASSET_NAME}" -C "$TMP_DIR"

# Create user, directories, and install binary.
echo "Creating user and directories..."
if ! id "$USER_NAME" &>/dev/null; then
    useradd --system --no-create-home --home-dir "$DATA_DIR" "$USER_NAME"
fi

mkdir -p "$DATA_DIR" "$CONFIG_DIR"
cp "${TMP_DIR}/vico-vee" "$INSTALL_DIR/vico-vee"
chmod 755 "$INSTALL_DIR/vico-vee"
chown -R "${USER_NAME}:${USER_NAME}" "$DATA_DIR"
chown -R "${USER_NAME}:${USER_NAME}" "$CONFIG_DIR"

# Generate an initial API key if one does not already exist.
KEYS_FILE="${CONFIG_DIR}/api_keys.toml"
if [[ ! -f "$KEYS_FILE" ]]; then
    API_KEY=$(openssl rand -hex 32 2>/dev/null || head -c 64 /dev/urandom | xxd -p | tr -d '\n')
    cat > "$KEYS_FILE" <<EOF
# vico-vee API keys
# Format: [keys.<name>] token = "..." scopes = ["submit", "read", "admin"]
[keys.admin]
token = "${API_KEY}"
scopes = ["submit", "read", "admin"]
EOF
    chmod 600 "$KEYS_FILE"
    chown "${USER_NAME}:${USER_NAME}" "$KEYS_FILE"
    echo "Generated initial API key: ${API_KEY}"
fi

# Install systemd unit when systemd is available.
if command -v systemctl &>/dev/null; then
    SYSTEMD_DIR="/etc/systemd/system"
    cp "${TMP_DIR}/systemd/vico-vee.service" "${SYSTEMD_DIR}/vico-vee.service"
    chmod 644 "${SYSTEMD_DIR}/vico-vee.service"
    systemctl daemon-reload
    echo "Installed systemd unit. Enable with: systemctl enable --now vico-vee"
else
    echo "systemd not detected. Skipping systemd unit installation."
fi

echo "vico-vee installed to ${INSTALL_DIR}/vico-vee"
