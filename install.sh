#!/bin/sh
# Installs sentrypanel from the latest release. After that the utility looks
# after itself: sentrypanel update to update, sentrypanel uninstall to remove.
#
#   curl -fsSL https://raw.githubusercontent.com/ivan-moskvin/sentrypanel/main/install.sh | sh
set -eu

REPO="ivan-moskvin/sentrypanel"
BIN_DIR="${SENTRYPANEL_BIN_DIR:-$HOME/.local/bin}"

os="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$os" in
    darwin|linux) ;;
    *) printf 'Unknown system: %s — build it from source.\n' "$os" >&2; exit 1 ;;
esac

case "$(uname -m)" in
    x86_64|amd64) arch=amd64 ;;
    arm64|aarch64) arch=arm64 ;;
    *) printf 'Unknown architecture: %s — build it from source.\n' "$(uname -m)" >&2; exit 1 ;;
esac

asset="sentrypanel_${os}_${arch}"
base="https://github.com/$REPO/releases/latest/download"

# Linux has sha256sum, macOS has shasum.
if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    printf 'Neither sha256sum nor shasum found — nothing to verify the download with.\n' >&2
    exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

printf '==> Downloading %s\n' "$asset"
curl -fsSL "$base/$asset" -o "$tmp/sentrypanel"
curl -fsSL "$base/checksums.txt" -o "$tmp/checksums.txt"

want="$(grep " \*\{0,1\}$asset\$" "$tmp/checksums.txt" | cut -d' ' -f1)"
if [ -z "$want" ]; then
    printf 'The release has no checksum for %s.\n' "$asset" >&2
    exit 1
fi
if [ "$want" != "$(sha256 "$tmp/sentrypanel")" ]; then
    printf 'The checksum does not match — the download is broken.\n' >&2
    exit 1
fi

mkdir -p "$BIN_DIR"
chmod +x "$tmp/sentrypanel"
mv "$tmp/sentrypanel" "$BIN_DIR/sentrypanel"
printf '==> Installed to %s\n' "$BIN_DIR/sentrypanel"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) printf '==> %s is not in PATH — add it to your shell profile:\n    export PATH="%s:$PATH"\n' "$BIN_DIR" "$BIN_DIR" ;;
esac

# A service installed earlier still runs the previous binary: installing it
# again restarts it on this one.
if [ -f "$HOME/Library/LaunchAgents/io.github.ivan-moskvin.sentrypanel.plist" ] ||
   [ -f "$HOME/.config/systemd/user/sentrypanel.service" ]; then
    "$BIN_DIR/sentrypanel" service install
    exit 0
fi

config="$("$BIN_DIR/sentrypanel" config 2>/dev/null)"
printf '\nNext:\n'
if [ ! -f "$config" ]; then
    printf '  sentrypanel config init       create %s\n' "$config"
fi
printf '  sentrypanel once              check the config and the token\n'
printf '  sentrypanel service install   start the panel at login, and now\n'
