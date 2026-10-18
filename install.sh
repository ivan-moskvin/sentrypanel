#!/bin/sh
# Installs sentrypanel from the latest release. After that the utility looks
# after itself: sentrypanel update to update, sentrypanel uninstall to remove.
#
#   curl -fsSL https://raw.githubusercontent.com/ivan-moskvin/sentrypanel/main/install.sh | sh
#
# With a bundle made by `sentrypanel config export` it also takes the config
# and the token from it, checks them and starts the panel:
#
#   curl -fsSL .../install.sh | SENTRYPANEL_BUNDLE=~/sentrypanel-bundle sh
#   sh install.sh ~/sentrypanel-bundle
set -eu

REPO="ivan-moskvin/sentrypanel"
BIN_DIR="${SENTRYPANEL_BIN_DIR:-$HOME/.local/bin}"
BUNDLE="${1:-${SENTRYPANEL_BUNDLE:-}}"

if [ -n "$BUNDLE" ] && [ ! -f "$BUNDLE" ]; then
    printf 'No bundle at %s.\n' "$BUNDLE" >&2
    exit 1
fi

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

# The bundle holds the token: once it is in place, the copy is better gone.
offer_to_delete() {
    answer=y
    if [ -r /dev/tty ]; then
        printf '==> Delete the bundle %s now? [Y/n] ' "$1"
        read -r answer </dev/tty || answer=y
    fi
    case "$answer" in
        n|N|no|NO) printf '==> Kept %s — it holds the token, delete it yourself\n' "$1" ;;
        *) rm -f "$1" && printf '==> Deleted %s\n' "$1" ;;
    esac
}

if [ -n "$BUNDLE" ]; then
    "$BIN_DIR/sentrypanel" config import "$BUNDLE"
    printf '==> Checking the config and the token\n'
    if ! "$BIN_DIR/sentrypanel" once; then
        printf 'The check failed — fix the config and run: sentrypanel once && sentrypanel service install\n' >&2
        exit 1
    fi
    "$BIN_DIR/sentrypanel" service install
    offer_to_delete "$BUNDLE"
    exit 0
fi

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
