#!/usr/bin/env bash
# End-to-end smoke test for install.sh.
#
# Builds a synthetic GitHub release on localhost, then drives install.sh
# against it via BBR_INSTALL_RELEASE_BASE / BBR_INSTALL_API_BASE. Covers the
# happy path and every fail-closed branch.
#
# Usage: scripts/install-smoke.sh [path-to-release-binary]
# Run `cargo build --release` first (default binary: target/release/bbr).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${1:-${REPO_ROOT}/target/release/bbr}"
PORT="${BBR_SMOKE_PORT:-8099}"
TAG="v0.2.5"

if [ ! -x "$BIN" ]; then
  echo "error: release binary not found at $BIN" >&2
  echo "  run: cargo build --release" >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
  *) echo "skipping: unsupported test arch $(uname -m)"; exit 0 ;;
esac
TARGET="${ARCH}-unknown-linux-musl"

FIX="$(mktemp -d)"
FAKE_HOME="$(mktemp -d)"
SERVER_PID=""
cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$FIX" "$FAKE_HOME"
}
trap cleanup EXIT

pass=0
fail=0
ok()   { echo "  PASS  $1"; pass=$((pass + 1)); }
bad()  { echo "  FAIL  $1" >&2; fail=$((fail + 1)); }

# ---- synthetic release -----------------------------------------------------
mkdir -p "$FIX/releases/download/${TAG}" "$FIX/dist"
cp "$BIN" "$FIX/dist/bbr"
cp "$REPO_ROOT/README.md" "$REPO_ROOT/LICENSE" "$FIX/dist/" 2>/dev/null || true
tar -czf "$FIX/releases/download/${TAG}/bbr-${TARGET}.tar.gz" -C "$FIX/dist" .
(cd "$FIX/releases/download/${TAG}" && sha256sum "bbr-${TARGET}.tar.gz" > checksums.txt)
printf '{"tag_name":"%s"}\n' "$TAG" > "$FIX/releases/latest"

python3 -m http.server "$PORT" --directory "$FIX" >/dev/null 2>&1 &
SERVER_PID=$!
sleep 1

export BBR_INSTALL_RELEASE_BASE="http://127.0.0.1:${PORT}/releases"
export BBR_INSTALL_API_BASE="http://127.0.0.1:${PORT}/releases"
export HOME="$FAKE_HOME"
mkdir -p "$HOME/.local/bin"
export PATH="$HOME/.local/bin:$PATH"

run_install() { bash "$REPO_ROOT/install.sh" "$@"; }

# ---- 1. happy path ---------------------------------------------------------
echo "1. happy path"
if run_install >"$FIX/log" 2>&1; then
  grep -q 'Checksum verified' "$FIX/log" && ok "verifies checksum" || bad "no checksum verification"
  if "$HOME/.local/bin/bbr" --version >/dev/null 2>&1; then
    ok "installed binary runs"
  else
    bad "installed binary does not run"
  fi
else
  bad "install failed: $(tail -1 "$FIX/log")"
fi

# ---- 2. checksum mismatch must abort ---------------------------------------
echo "2. corrupted checksum fails closed"
(cd "$FIX/releases/download/${TAG}" \
  && printf '%s  %s\n' "$(printf 'ab%.0s' {1..32})" "bbr-${TARGET}.tar.gz" > checksums.txt)
if run_install >"$FIX/log" 2>&1; then
  bad "accepted a corrupted checksum"
else
  grep -qi 'checksum mismatch' "$FIX/log" \
    && ok "aborts with a mismatch error" || bad "aborted without a clear reason"
fi

# ---- 3. missing checksums.txt must abort -----------------------------------
echo "3. missing checksums.txt fails closed"
mv "$FIX/releases/download/${TAG}/checksums.txt" "$FIX/releases/download/${TAG}/checksums.hidden"
if run_install >"$FIX/log" 2>&1; then
  bad "installed with no checksum file"
else
  grep -qi 'refusing to install' "$FIX/log" \
    && ok "aborts asking for an override" || bad "aborted without an actionable message"
fi

# ---- 4. explicit override --------------------------------------------------
echo "4. BBR_SKIP_CHECKSUM=1 proceeds"
if BBR_SKIP_CHECKSUM=1 run_install >"$FIX/log" 2>&1; then
  grep -qi 'warning' "$FIX/log" && ok "warns before skipping verification" || bad "no warning"
else
  bad "override did not work"
fi
mv "$FIX/releases/download/${TAG}/checksums.hidden" "$FIX/releases/download/${TAG}/checksums.txt"

# ---- 5. unknown tag --------------------------------------------------------
echo "5. unknown tag fails with a link"
if run_install v9.9.9 >"$FIX/log" 2>&1; then
  bad "succeeded for a nonexistent tag"
else
  grep -q 'releases/tag/v9.9.9' "$FIX/log" \
    && ok "reports the release URL" || bad "no release URL in the error"
fi

# ---- 6. unreachable API ----------------------------------------------------
echo "6. unreachable API gives an actionable message"
if HOME="$HOME" PATH="$PATH" \
   BBR_INSTALL_RELEASE_BASE="http://127.0.0.1:1/releases" \
   BBR_INSTALL_API_BASE="http://127.0.0.1:1/releases" \
   run_install >"$FIX/log" 2>&1; then
  bad "succeeded with an unreachable API"
else
  grep -q 'GITHUB_TOKEN' "$FIX/log" \
    && ok "suggests a token / explicit version" || bad "no remediation hint"
fi

echo
echo "install.sh smoke: ${pass} passed, ${fail} failed"
[ "$fail" -eq 0 ]
