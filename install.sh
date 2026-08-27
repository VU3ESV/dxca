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
#
# Building needs rustc >= $MIN_RUSTC (checked up front, see require_cargo)
# and, for the real web UI rather than the stub page, Node >= 20 + pnpm.
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
    say "Building web UI (pnpm)..."
    pnpm -C web-ui install && pnpm -C web-ui build
  else
    say "NOTE: pnpm not found — the binary will embed whatever web-ui/dist"
    say "holds (a stub page on a fresh clone). Install Node ≥ 20 + pnpm and"
    say "re-run for the full dashboard."
  fi
}

# Minimum rustc. The floor is set by the committed Cargo.lock, not by our
# own code: ureq -> url -> idna -> idna_adapter -> icu_* 2.3.0 all require
# 1.88. Nothing in the manifests declares a rust-version, so without this
# check the only complaint comes from cargo minutes into dependency
# resolution — and Debian Trixie's apt rustc is 1.85.0, so a plain
# `apt install cargo` lands under the floor on a fresh Pi. Bump this when
# the lockfile's floor moves.
MIN_RUSTC=1.88

rust_install_hint() {
  if [ "$PLATFORM" = macos ]; then
    say "  brew install rustup && rustup-init          # recommended"
  else
    say "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
    say "  source \"\$HOME/.cargo/env\""
  fi
}

# "rustc 1.85.0 (4d91de4e4 2025-02-17)" -> "1.85.0". The `cut` drops a
# channel suffix so 1.99.0-nightly still compares as 1.99.
rustc_version() {
  command -v rustc >/dev/null 2>&1 || return 1
  rustc --version 2>/dev/null | awk 'NR==1 {print $2}' | cut -d- -f1
}

# major.minor only — a patch level is never part of an MSRV floor.
version_ok() {
  awk -v have="$1" -v min="$2" 'BEGIN {
    split(have, h, "."); split(min, m, ".");
    exit !(h[1] > m[1] || (h[1] == m[1] && h[2] >= m[2]))
  }'
}

require_cargo() {
  if ! command -v cargo >/dev/null 2>&1; then
    say "cargo not found. Install Rust first:"
    rust_install_hint
    die "then re-run this script"
  fi
  RUSTC_HAVE="$(rustc_version || true)"
  if [ -z "$RUSTC_HAVE" ]; then
    say "cargo is on PATH but 'rustc --version' returned nothing usable."
    say "A half-installed toolchain builds nothing — repair it first:"
    rust_install_hint
    die "then re-run this script"
  fi
  if ! version_ok "$RUSTC_HAVE" "$MIN_RUSTC"; then
    say "rustc $RUSTC_HAVE is too old: this workspace needs $MIN_RUSTC or newer."
    say "The floor comes from Cargo.lock (ureq -> url -> idna -> icu_*),"
    say "not from dxca's own code, so it cannot be worked around here."
    say "Found: $(command -v rustc)"
    if command -v rustup >/dev/null 2>&1; then
      say "rustup is installed, so the toolchain is merely stale:"
      say "  rustup update stable"
    else
      say "That is a distro package (Debian Trixie ships 1.85.0) and it will"
      say "never honour rust-toolchain.toml. Install rustup instead:"
      rust_install_hint
      say "Then confirm 'which rustc' is ~/.cargo/bin, not /usr/bin."
    fi
    die "then re-run this script"
  fi
  say "rustc $RUSTC_HAVE (needs $MIN_RUSTC+) - OK."
}

case "$PLATFORM" in
  macos)
    require_cargo
    build_web
    say "Building release binary..."
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
      say "Building release binary (native — this takes a while on a Pi)..."
      cargo build --release -p dxca-server
      BIN="$REPO/target/release/dxca"
    fi
    # The service runs as whoever invokes this script — no hardcoded user.
    SERVICE_USER="$(id -un)"
    SERVICE_GROUP="$(id -gn)"
    say "Installing to /opt/dxca (service user: $SERVICE_USER)..."
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
    sudo systemctl enable dxca
    # `enable --now` starts an inactive unit but does NOTHING to an active
    # one, so re-installing over a running service left the OLD process
    # holding the replaced binary's inode — the new build silently never
    # ran. Restart unconditionally: this script has just written the binary
    # it is meant to be running.
    sudo systemctl restart dxca
    say "Installed systemd service 'dxca' (status: systemctl status dxca)."
    say "Web UI: http://$(hostname -I 2>/dev/null | awk '{print $1}'):7580/"
    ;;

  *)
    die "unsupported platform '$PLATFORM' (use: macos / pi / linux)"
    ;;
esac
