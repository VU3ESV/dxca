#!/usr/bin/env bash
# DXCA installer — sets this machine up to run dxca as a service.
# Shack rule (~/.claude/CLAUDE.md): install scripts support both macOS
# (daily driver) and Raspberry Pi (always-on host), auto-detect, confirm,
# and never fail silently.
#
#   install.sh [macos|pi|linux] [--stub-ui]
#
#   macOS : builds the dashboard + release binary and installs a launchd
#           agent (survives reboots).
#   Pi    : builds the same, or uses the prebuilt ./dxca from a
#           pi-deploy.sh bundle; installs to /opt/dxca + a systemd
#           service (needs sudo).
#
#   --stub-ui   proceed without pnpm, embedding build.rs's placeholder
#               page instead of the dashboard. Without it a missing pnpm
#               is a hard stop: the web GUI is part of what "install"
#               means here.
#
# Needs rustc >= MIN_RUSTC (checked up front, see require_cargo) and, for
# the dashboard, Node >= 20 + pnpm. In a source tree the binary is ALWAYS
# rebuilt, so a re-run after installing pnpm really does pick the
# dashboard up — the embed happens at compile time.
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

STUB_UI=0
PLATFORM=""
for arg in "$@"; do
  case "$arg" in
    --stub-ui) STUB_UI=1 ;;
    # Print the whole header comment, however long it grows — a hardcoded
    # line range silently truncates the help the next time it is edited.
    -h|--help) awk 'NR>1 && /^#/ {sub(/^# ?/, ""); print; next} NR>1 {exit}' "$0"; exit 0 ;;
    -*) die "unknown option '$arg' (try --help)" ;;
    *) PLATFORM="$arg" ;;
  esac
done

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

# The dashboard is embedded at COMPILE time (include_dir over web-ui/dist),
# so "install the web GUI" means "build dist, then build the binary" — in
# that order, every time. Without pnpm the binary still links, but what it
# serves is build.rs's placeholder page. For `cargo build` that is the right
# trade (no Node required, Meridian rule); for an INSTALLER it is a silent
# failure, so this stops unless --stub-ui says the placeholder is wanted.
build_web() {
  if command -v pnpm >/dev/null 2>&1; then
    say "Building web UI (pnpm)..."
    pnpm -C web-ui install && pnpm -C web-ui build
    return 0
  fi
  if [ "$STUB_UI" -eq 1 ]; then
    say "NOTE: --stub-ui and no pnpm — embedding the placeholder page, not"
    say "the dashboard. The API and telnet server are unaffected."
    return 0
  fi
  say "pnpm not found, so the dashboard cannot be built. The binary would"
  say "still run, serving a placeholder page instead of the web GUI — which"
  say "for an installer is a failure, not a warning."
  say ""
  # Print the command that fits THIS box. The obvious one-liner
  # `apt install -y nodejs npm` is wrong wherever Node came from NodeSource:
  # that nodejs package provides its own npm and declares Conflicts: npm, so
  # apt refuses with a wall of unsatisfiable node-* dependencies. Seen on
  # VU2WJ's Pi (Trixie + nodesource 22.23.2). Use what is already installed.
  if command -v npm >/dev/null 2>&1; then
    say "Node $(node --version 2>/dev/null || echo '?') and npm are already here, so:"
    say "  sudo npm install -g pnpm"
  elif command -v corepack >/dev/null 2>&1; then
    say "Node ships corepack, so:"
    say "  sudo corepack enable pnpm"
  elif [ "$PLATFORM" = macos ]; then
    say "Install Node >= 20 and pnpm:"
    say "  brew install node pnpm"
  else
    say "Install Node >= 20, then pnpm with the npm it brings:"
    say "  sudo apt install -y nodejs      # NOT 'npm' — nodejs provides it,"
    say "                                  # and NodeSource's conflicts with it"
    say "  sudo npm install -g pnpm"
  fi
  say ""
  die "then re-run, or pass --stub-ui to accept the placeholder"
}

# Minimum rustc. The floor is set by the committed Cargo.lock, not by our
# own code: ureq -> url -> idna -> idna_adapter -> icu_* 2.3.0 all require
# 1.88. Debian Trixie's apt rustc is 1.85.0, so a plain `apt install cargo`
# lands under the floor on a fresh Pi.
#
# `rust-version = "1.88"` in [workspace.package] now makes cargo refuse
# early on its own, so this is no longer the only guard. It stays because
# it fires before the pnpm web build and before the first sudo, and because
# it can name WHICH remedy applies — a stale rustup versus a distro package
# that will never honour rust-toolchain.toml — which cargo cannot.
# Keep this constant in step with the manifest's rust-version.
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
    # Two shapes of install, and conflating them is what broke the web GUI:
    #
    #   source tree (git clone) -> ALWAYS rebuild. This used to prefer an
    #     existing target/release/dxca over building, so a re-run after
    #     installing pnpm reused the stale binary and went on serving the
    #     placeholder page — install.sh appearing not to install the web GUI
    #     at all. cargo is incremental; rebuilding an unchanged tree is cheap.
    #   pi-deploy.sh bundle -> no crates/, no web-ui/, just ./dxca that was
    #     cross-compiled on the Mac with the dashboard already embedded.
    if [ -d "$REPO/crates" ]; then
      require_cargo
      build_web
      say "Building release binary (native — this takes a while on a Pi)..."
      cargo build --release -p dxca-server
      BIN="$REPO/target/release/dxca"
    elif [ -x "$REPO/dxca" ]; then
      say "Using the prebuilt binary shipped in this bundle."
      BIN="$REPO/dxca"
    else
      die "no crates/ to build from and no prebuilt ./dxca — nothing to install"
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
