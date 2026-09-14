#!/usr/bin/env bash
# bbr – one-line install
#
#   curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash
#   curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash -s v0.2.5
#
# Pass a version as the first argument to skip the GitHub API lookup (useful
# in CI, where the unauthenticated API is rate-limited to 60 req/hr/IP).
#
# Integrity: the download is verified against the release's `checksums.txt`
# and FAILS CLOSED. Set BBR_SKIP_CHECKSUM=1 to opt out (not recommended).
set -euo pipefail

APP="bbr"
REPO="themankindproject/bbr"
VERSION="${1:-latest}"
SKIP_CHECKSUM="${BBR_SKIP_CHECKSUM:-}"

# Release/API endpoints. Overridable so the install path can be exercised
# against a local fixture in CI without reaching GitHub.
RELEASE_BASE="${BBR_INSTALL_RELEASE_BASE:-https://github.com/${REPO}/releases}"
API_BASE="${BBR_INSTALL_API_BASE:-https://api.github.com/repos/${REPO}/releases}"

# --proto '=https' is enforced only for the real endpoints; a local http
# fixture would be rejected otherwise.
CURL_OPTS=(--fail --silent --show-error --location)
case "$RELEASE_BASE$API_BASE" in
  https://*) CURL_OPTS=(--proto '=https' --tlsv1.2 "${CURL_OPTS[@]}") ;;
esac

# ---- platform detection ----------------------------------------------------
PLATFORM="$(uname -s)"
ARCH="$(uname -m)"

# Git-Bash / MSYS2 / Cygwin report MINGW64_NT-*, MSYS_NT-*, CYGWIN_NT-*.
case "$PLATFORM" in
  Linux)   OS="unknown-linux";  SHELL_OS="unix" ;;
  Darwin)  OS="apple-darwin";   SHELL_OS="unix" ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT)
    OS="pc-windows"; SHELL_OS="windows" ;;
  *) echo "unsupported platform: $PLATFORM" >&2; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)  ARCH="x86_64"  ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

if [ "$SHELL_OS" = "windows" ]; then
  # Only the MSVC target is published for Windows (x86_64 today).
  TARGETS="${ARCH}-pc-windows-msvc"
  EXT="zip"
else
  # Prefer the fully-static musl build on Linux (no glibc version floor, works
  # on Alpine); fall back to the gnu build for older releases.
  EXT="tar.gz"
  if [ "$OS" = "unknown-linux" ]; then
    TARGETS="${ARCH}-unknown-linux-musl ${ARCH}-unknown-linux-gnu"
  else
    TARGETS="${ARCH}-apple-darwin"
  fi
fi

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "error: need sha256sum or shasum to verify download integrity" >&2
    exit 1
  fi
}

# ---- resolve release tag ---------------------------------------------------
AUTH_HEADER=()
if [ -n "${GITHUB_TOKEN:-}" ]; then
  AUTH_HEADER=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
elif [ -n "${GH_TOKEN:-}" ]; then
  AUTH_HEADER=(-H "Authorization: Bearer ${GH_TOKEN}")
fi

if [ "$VERSION" = "latest" ]; then
  API="${API_BASE}/latest"
  # Capture the response first and tolerate transport failure, so an
  # unreachable API prints the actionable message below instead of aborting
  # on `set -e` with curl's raw exit code.
  RESP="$(curl "${CURL_OPTS[@]}" "${AUTH_HEADER[@]}" "$API" 2>/dev/null)" || RESP=""
  TAG="$(printf '%s' "$RESP" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
  if [ -z "$TAG" ]; then
    echo "error: could not resolve the latest release of ${REPO}." >&2
    echo "  This is usually GitHub API rate limiting on shared IPs." >&2
    echo "  Retry, set GITHUB_TOKEN, or pass a version explicitly:" >&2
    echo "    curl -fsSL .../install.sh | bash -s v0.2.5" >&2
    exit 1
  fi
else
  TAG="$VERSION"
fi

case "$TAG" in
  v[0-9]*) : ;;
  *) echo "error: '${TAG}' does not look like a release tag (expected vX.Y.Z)" >&2; exit 1 ;;
esac

# ---- download & verify -----------------------------------------------------
TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

CHECKSUMS_URL="${RELEASE_BASE}/download/${TAG}/checksums.txt"
CHECKSUMS_OK=0
if curl "${CURL_OPTS[@]}" "$CHECKSUMS_URL" -o "$TMP/checksums.txt" 2>/dev/null; then
  CHECKSUMS_OK=1
fi

ARCHIVE=""
for T in $TARGETS; do
  CANDIDATE="${APP}-${T}.${EXT}"
  echo "Downloading ${APP} ${TAG} (${T})..."
  if curl "${CURL_OPTS[@]}" \
      "${RELEASE_BASE}/download/${TAG}/${CANDIDATE}" \
      -o "$TMP/${CANDIDATE}" 2>/dev/null; then
    ARCHIVE="$CANDIDATE"
    break
  fi
  echo "  no asset ${CANDIDATE}; trying next"
done

if [ -z "$ARCHIVE" ]; then
  echo "error: no release asset found for ${ARCH} on ${PLATFORM} in ${TAG}." >&2
  echo "  Tried: $(echo $TARGETS | sed 's/ /, /g')" >&2
  echo "  See https://github.com/${REPO}/releases/tag/${TAG}" >&2
  exit 1
fi

# Fail closed: a missing checksum is a hard error unless explicitly opted out.
if [ "$SKIP_CHECKSUM" = "1" ]; then
  echo "warning: BBR_SKIP_CHECKSUM=1 — installing without integrity verification." >&2
elif [ "$CHECKSUMS_OK" -ne 1 ]; then
  echo "error: could not download checksums.txt for ${TAG}; refusing to install" >&2
  echo "  an unverified binary. Re-run with BBR_SKIP_CHECKSUM=1 to override." >&2
  exit 1
else
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
    echo "error: checksums.txt has no entry for ${ARCHIVE}; refusing to install" >&2
    echo "  an unverified binary. Re-run with BBR_SKIP_CHECKSUM=1 to override." >&2
    exit 1
  fi

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

# ---- extract ---------------------------------------------------------------
echo "Extracting..."
mkdir -p "$TMP/extract"
if [ "$EXT" = "zip" ]; then
  command -v unzip >/dev/null 2>&1 || {
    echo "error: need 'unzip' to extract ${ARCHIVE}" >&2; exit 1; }
  unzip -q "$TMP/${ARCHIVE}" -d "$TMP/extract"
else
  tar -xzf "$TMP/${ARCHIVE}" -C "$TMP/extract"
fi

# The archive layout is not contractual (release.yml puts the binary at the
# root, but a future change could nest it) — search for it instead of assuming.
BIN_PATH="$(find "$TMP/extract" -type f \( -name "$APP" -o -name "${APP}.exe" \) | head -1)"
if [ -z "$BIN_PATH" ]; then
  echo "error: archive ${ARCHIVE} did not contain a ${APP} binary" >&2
  exit 1
fi

# ---- install ---------------------------------------------------------------
if [ "$SHELL_OS" = "windows" ]; then
  DEST="${LOCALAPPDATA:-$HOME}/bbr"
  mkdir -p "$DEST"
  cp "$BIN_PATH" "$DEST/${APP}.exe"
  echo "Installed ${APP} to ${DEST}\\${APP}.exe"
  echo "Add ${DEST} to your PATH if it is not already there."
  exit 0
fi

# Choose the install directory.
#
# Prefer a user-local bin that is already on PATH; otherwise prefer a
# writable user-local bin anyway (and warn afterwards) rather than escalating
# to a system directory. Only use /usr/local/bin when no user-local option is
# writable — that keeps the common one-liner working without sudo.
DEST=""
for candidate in "${HOME}/.local/bin" "${HOME}/bin"; do
  if [ -d "$candidate" ] && [ -w "$candidate" ]; then
    DEST="$candidate"
    [[ ":${PATH}:" == *":${candidate}:"* ]] && break
  fi
done

if [ -z "$DEST" ]; then
  if [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
    DEST="/usr/local/bin"
  else
    # Last resort: create ~/.local/bin rather than failing the install.
    DEST="${HOME}/.local/bin"
    mkdir -p "$DEST" 2>/dev/null || {
      echo "error: no writable install directory found." >&2
      echo "  Create ~/.local/bin and re-run, or install with sudo." >&2
      exit 1
    }
  fi
fi

install -m 0755 "$BIN_PATH" "$DEST/${APP}"
echo "Installed ${APP} to ${DEST}/${APP}"

# Tell the user if the destination is not on their PATH — otherwise the very
# next command they type is "bbr: command not found".
if [[ ":${PATH}:" != *":${DEST}:"* ]]; then
  # RC is display-only shell guidance, not a path used for file access.
  # shellcheck disable=SC2088
  case "$(basename "${SHELL:-bash}")" in
    zsh)  RC="~/.zshrc" ;;
    fish) RC="~/.config/fish/config.fish" ;;
    *)    RC="~/.bashrc" ;;
  esac
  echo
  echo "NOTE: ${DEST} is not on your PATH. Add it by appending this to ${RC}:"
  if [ "$(basename "${SHELL:-bash}")" = "fish" ]; then
    echo "    fish_add_path ${DEST}"
  else
    echo "    export PATH=\"${DEST}:\$PATH\""
  fi
  echo "Then restart your shell or run: source ${RC}"
fi

# ---- shell completions (optional) ------------------------------------------
# Prefer user-local completion dirs; fall back to system dirs only when
# writable (no hard `sudo` dependency — keeps one-liner installs working
# on machines without passwordless sudo).
if command -v "${APP}" >/dev/null 2>&1; then
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
