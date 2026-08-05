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

sha256_file() {
  if command -v sha256sum &>/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum &>/dev/null; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "error: need sha256sum or shasum to verify download integrity" >&2
    exit 1
  fi
}

# ---- resolve download URL --------------------------------------------------
if [ "$VERSION" = "latest" ]; then
  API="https://api.github.com/repos/${REPO}/releases/latest"
  TAG="$(curl -fsSL "$API" | grep '"tag_name"' | head -1 | sed 's/.*: "//;s/",//')"
else
  TAG="$VERSION"
fi

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"
CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${TAG}/checksums.txt"

# ---- download & install ----------------------------------------------------
TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

echo "Downloading ${APP} ${TAG} (${TARGET})…"
curl -fsSL "$DOWNLOAD_URL" -o "$TMP/${ARCHIVE}"

echo "Verifying checksum…"
if curl -fsSL "$CHECKSUMS_URL" -o "$TMP/checksums.txt"; then
  EXPECTED=""
  while read -r hash name; do
    case "$hash" in ''|\#*) continue ;; esac
    name="${name#\*}"
    if [ "$name" = "$ARCHIVE" ]; then
      EXPECTED="$hash"
      break
    fi
  done < "$TMP/checksums.txt"

  if [ -z "$EXPECTED" ]; then
    echo "Warning: no checksum entry for ${ARCHIVE}; skipping verification."
  else
    ACTUAL="$(sha256_file "$TMP/${ARCHIVE}")"
    if [ "$ACTUAL" != "$EXPECTED" ]; then
      echo "ERROR: SHA256 checksum mismatch for ${ARCHIVE}!" >&2
      echo "  Expected: ${EXPECTED}" >&2
      echo "  Got:      ${ACTUAL}" >&2
      echo "The download may be corrupted or tampered with. Aborting." >&2
      exit 1
    fi
    echo "Checksum verified."
  fi
else
  echo "Warning: checksums.txt not found for ${TAG}; skipping verification."
fi

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
# Prefer user-local completion dirs; fall back to system dirs only when
# writable (no hard `sudo` dependency — keeps one-liner installs working
# on machines without passwordless sudo).
if command -v "${APP}" &>/dev/null; then
  SHELLNAME="$(basename "${SHELL:-bash}")"
  case "$SHELLNAME" in
    bash)
      if [ -d "${HOME}/.local/share/bash-completion/completions" ]; then
        "${APP}" completion bash > "${HOME}/.local/share/bash-completion/completions/${APP}" 2>/dev/null || true
      elif [ -w "/usr/share/bash-completion/completions" ]; then
        "${APP}" completion bash > "/usr/share/bash-completion/completions/${APP}" 2>/dev/null || true
      fi
      ;;
    zsh)
      if [ -d "${HOME}/.zsh/completions" ]; then
        "${APP}" completion zsh > "${HOME}/.zsh/completions/_${APP}" 2>/dev/null || true
      elif [ -w "/usr/local/share/zsh/site-functions" ]; then
        "${APP}" completion zsh > "/usr/local/share/zsh/site-functions/_${APP}" 2>/dev/null || true
      fi
      ;;
    fish)
      mkdir -p "${HOME}/.config/fish/completions"
      "${APP}" completion fish > "${HOME}/.config/fish/completions/${APP}.fish" 2>/dev/null || true
      ;;
  esac
fi

echo "Run '${APP} --help' to get started."
