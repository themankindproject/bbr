#!/usr/bin/env bash
# bbr – one-line install
#   curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash
set -euo pipefail

APP="bbr"
REPO="themankindproject/bbr"
VERSION="${1:-latest}"

# ---- platform detection ----------------------------------------------------
PLATFORM="$(uname -s)"
ARCH="$(uname -m)"

case "$PLATFORM" in
  Linux)  OS="unknown-linux-gnu"    ;;
  Darwin) OS="apple-darwin"         ;;
  *)      echo "unsupported platform: $PLATFORM"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64"  ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *)           echo "unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${ARCH}-${OS}"
ARCHIVE="${APP}-${TARGET}.tar.gz"

# ---- resolve download URL --------------------------------------------------
if [ "$VERSION" = "latest" ]; then
  API="https://api.github.com/repos/${REPO}/releases/latest"
  TAG="$(curl -fsSL "$API" | grep '"tag_name"' | head -1 | sed 's/.*: "//;s/",//')"
else
  TAG="$VERSION"
fi

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"

# ---- download & install ----------------------------------------------------
TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

echo "Downloading ${APP} ${TAG} (${TARGET})…"
curl -fsSL "$DOWNLOAD_URL" -o "$TMP/${ARCHIVE}"

echo "Extracting…"
tar -xzf "$TMP/${ARCHIVE}" -C "$TMP"

# Prefer user-local bin, fall back to global
if [ -d "${HOME}/.local/bin" ] && [[ ":$PATH:" == *":${HOME}/.local/bin:"* ]]; then
  DEST="${HOME}/.local/bin"
elif [ -d "${HOME}/bin" ] && [[ ":$PATH:" == *":${HOME}/bin:"* ]]; then
  DEST="${HOME}/bin"
elif [ -w "/usr/local/bin" ]; then
  DEST="/usr/local/bin"
else
  echo "Cannot determine install directory. Add ~/.local/bin to PATH or run with sudo."
  exit 1
fi

install -m 0755 "$TMP/${APP}" "$DEST/${APP}"
echo "Installed ${APP} to ${DEST}/${APP}"

# ---- shell completions (optional) ------------------------------------------
if command -v "${APP}" &>/dev/null; then
  SHELLNAME="$(basename "${SHELL:-bash}")"
  case "$SHELLNAME" in
    bash) "${APP}" completion bash | sudo tee /usr/share/bash-completion/completions/"${APP}" &>/dev/null || true ;;
    zsh)  "${APP}" completion zsh  | sudo tee /usr/local/share/zsh/site-functions/_"${APP}" &>/dev/null || true ;;
    fish) "${APP}" completion fish > "${HOME}/.config/fish/completions/${APP}.fish" 2>/dev/null || true ;;
  esac
fi

echo "Run '${APP} --help' to get started."
