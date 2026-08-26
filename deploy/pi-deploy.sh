#!/usr/bin/env bash
# Cross-compile on the Mac and ship dxca to a Pi as a systemd service —
# the "one binary + one TOML" deploy from docs/PLAN.md §9.
#
#   deploy/pi-deploy.sh [user@host]      (default vu2cpl@noderedpi4.local —
#                                         the shack Pi; pass your own for
#                                         any other target)
#
# Ships: the aarch64 binary, deploy/dxca.service, install.sh, and — only
# when the Pi doesn't have them yet — config/dxca.toml and data/ seeds
# (cty.xml, lotw-users.txt, dxca.db). Then runs install.sh pi remotely.
set -euo pipefail

HOST="${1:-vu2cpl@noderedpi4.local}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

echo "Building web UI + aarch64 binary…"
pnpm -C web-ui install && pnpm -C web-ui build
cargo zigbuild --release -p dxca-server --target aarch64-unknown-linux-gnu.2.36

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/deploy" "$STAGE/config" "$STAGE/data"
cp target/aarch64-unknown-linux-gnu/release/dxca "$STAGE/dxca"
cp deploy/dxca.service "$STAGE/deploy/"
cp deploy/com.vu2cpl.dxca.plist "$STAGE/deploy/" 2>/dev/null || true
cp install.sh "$STAGE/"
[ -f config/dxca.toml ] && cp config/dxca.toml "$STAGE/config/"
for f in cty.xml lotw-users.txt dxca.db; do
  [ -f "data/$f" ] && cp "data/$f" "$STAGE/data/"
done

echo "Shipping to $HOST…"
ssh "$HOST" 'mkdir -p ~/dxca-deploy'
rsync -a --delete "$STAGE/" "$HOST:dxca-deploy/"
ssh "$HOST" 'cd ~/dxca-deploy && bash install.sh pi'
echo "Done. Check: ssh $HOST systemctl status dxca"
