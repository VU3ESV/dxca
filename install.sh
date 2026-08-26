#!/usr/bin/env bash
# DXCA installer — sets this machine up to run dxca as a service.
# Shack rule (~/.claude/CLAUDE.md): install scripts support both macOS
# (daily driver) and Raspberry Pi (always-on host), auto-detect, confirm,
# and never fail silently.
#
#   macOS : builds the release binary (web UI included when pnpm exists)
#           and installs a launchd agent (survives reboots).
#   Pi    : installs the binary + config + data to /opt/dxca and a systemd
#           service (needs sudo). Uses a prebuilt ./dxca or target/ binary
#           when present, else builds with cargo.
set -euo pipefail

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

# --- platform detection (auto + confirm, manual override as $1) ----------
detect() {
  case "$(uname -s)" in
    Darwin) echo macos ;;
    Linux)
      if grep -qiE 'raspberry pi|bcm27' /proc/cpuinfo /proc/device-tree/model 2>/dev/null; then
        echo pi
      else
        echo linux
      fi ;;
    *) echo unknown ;;
  esac
}

PLATFORM="${1:-}"
if [ -z "$PLATFORM" ]; then
  PLATFORM="$(detect)"
  printf 'Auto-detected platform: %s. Is this correct? [Y/n] ' "$PLATFORM"
  read -r answer
  case "${answer:-Y}" in
    Y|y|'') ;;
    *) printf 'Enter platform (macos / pi / linux): '; read -r PLATFORM ;;
  esac
fi

REPO="$(cd "$(dirname "$0")" && pwd)"
cd "$REPO"

build_web() {
  if command -v pnpm >/dev/null 2>&1; then
    say "Building web UI (pnpm)…"
    pnpm -C web-ui install && pnpm -C web-ui build
  else
    say "NOTE: pnpm not found — the binary will embed whatever web-ui/dist"
    say "holds (a stub page on a fresh clone). Install Node ≥ 20 + pnpm and"
    say "re-run for the full dashboard."
  fi
}

require_cargo() {
  command -v cargo >/dev/null 2>&1 && return 0
  say "cargo not found. Install Rust first:"
  if [ "$PLATFORM" = macos ]; then
    say "  brew install rustup && rustup-init          # recommended"
  else
    say "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  fi
  die "then re-run this script"
}

case "$PLATFORM" in
  macos)
    require_cargo
    build_web
    say "Building release binary…"
    cargo build --release -p dxca-server
    PLIST="$HOME/Library/LaunchAgents/com.vu2cpl.dxca.plist"
    mkdir -p "$HOME/Library/LaunchAgents"
    sed -e "s|__REPO__|$REPO|g" -e "s|__HOME__|$HOME|g" \
      deploy/com.vu2cpl.dxca.plist > "$PLIST"
    # Reload cleanly whether or not an older agent is present.
    launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
    pkill -f 'target/release/dxca' 2>/dev/null || true
    sleep 1
    launchctl bootstrap "gui/$(id -u)" "$PLIST"
    say "Installed launchd agent com.vu2cpl.dxca (log: ~/Library/Logs/dxca.log)."
    say "Web UI: http://localhost:7580/"
    ;;

  pi|linux)
    command -v sudo >/dev/null 2>&1 || die "sudo is required to install the systemd service"
    # Prefer a prebuilt binary (shipped by deploy/pi-deploy.sh) over building.
    BIN=""
    for candidate in "$REPO/dxca" "$REPO/target/release/dxca"; do
      [ -x "$candidate" ] && BIN="$candidate" && break
    done
    if [ -z "$BIN" ]; then
      require_cargo
      build_web
      say "Building release binary (native — this takes a while on a Pi)…"
      cargo build --release -p dxca-server
      BIN="$REPO/target/release/dxca"
    fi
    # The service runs as whoever invokes this script — no hardcoded user.
    SERVICE_USER="$(id -un)"
    SERVICE_GROUP="$(id -gn)"
    say "Installing to /opt/dxca (service user: $SERVICE_USER)…"
    sudo mkdir -p /opt/dxca/config /opt/dxca/data
    sudo install -m 755 "$BIN" /opt/dxca/dxca
    # Config/data are seeded only if absent — never clobber a live install.
    if [ ! -f /opt/dxca/config/dxca.toml ] && [ -f "$REPO/config/dxca.toml" ]; then
      sudo install -m 644 "$REPO/config/dxca.toml" /opt/dxca/config/dxca.toml
    fi
    for f in cty.xml lotw-users.txt dxca.db; do
      if [ ! -f "/opt/dxca/data/$f" ] && [ -f "$REPO/data/$f" ]; then
        sudo install -m 600 "$REPO/data/$f" "/opt/dxca/data/$f"
      fi
    done
    sudo chown -R "$SERVICE_USER:$SERVICE_GROUP" /opt/dxca
    sed "s|__USER__|$SERVICE_USER|g" "$REPO/deploy/dxca.service" \
      | sudo tee /etc/systemd/system/dxca.service >/dev/null
    sudo systemctl daemon-reload
    sudo systemctl enable --now dxca
    say "Installed systemd service 'dxca' (status: systemctl status dxca)."
    say "Web UI: http://$(hostname -I 2>/dev/null | awk '{print $1}'):7580/"
    ;;

  *)
    die "unsupported platform '$PLATFORM' (use: macos / pi / linux)"
    ;;
esac
