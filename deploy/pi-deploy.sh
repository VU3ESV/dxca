#!/usr/bin/env bash
# Cross-compile on the Mac and ship dxca to a Pi as a systemd service —
# the "one binary + one TOML" deploy from docs/PLAN.md §9.
#
#   deploy/pi-deploy.sh [--no-seed] [user@host]
#
#     user@host   default vu2cpl@noderedpi4.local (the shack Pi). Over a VPN
#                 use the IP — mDNS `.local` names generally do not resolve
#                 across the tunnel.
#     --no-seed   ship ONLY the binary, the service unit and install.sh.
#
# Ships by default: the aarch64 binary, deploy/dxca.service, install.sh, and
# — only when the Pi doesn't have them yet — config/dxca.toml and the data/
# seeds (cty.xml, lotw-users.txt, dxca.db). Then runs install.sh pi remotely.
#
# WHY --no-seed EXISTS. That seeding is a convenience for re-deploying to a
# box that already has its own files (the guard makes it a no-op there). On a
# FRESH host it is the opposite of harmless, because nothing is present to
# guard against:
#
#   * data/dxca.db carries ClubLog app passwords, API keys and the Telegram
#     bot token IN PLAIN TEXT (by design — see README §Secrets), plus account
#     password hashes. Seeding it onto someone else's Pi hands them over.
#   * config/dxca.toml carries the cluster nodes with THIS station's
#     login_call. A second host dialling the same nodes with the same
#     callsign makes the two fight over the session — DXSpider kicks
#     duplicate logins, so both ends flap.
#
# So: --no-seed for any Pi that is not this shack's. The remote box then
# self-bootstraps — first-run setup card for the admin account, cty.xml and
# the LoTW list downloaded on demand.
set -euo pipefail

NO_SEED=0
HOST=""
for arg in "$@"; do
  case "$arg" in
    --no-seed) NO_SEED=1 ;;
    -h|--help) sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "pi-deploy: unknown option '$arg'" >&2; exit 2 ;;
    *) HOST="$arg" ;;
  esac
done
HOST="${HOST:-vu2cpl@noderedpi4.local}"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

echo "Building web UI + aarch64 binary..."
pnpm -C web-ui install && pnpm -C web-ui build
cargo zigbuild --release -p dxca-server --target aarch64-unknown-linux-gnu.2.36

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/deploy" "$STAGE/config" "$STAGE/data"
cp target/aarch64-unknown-linux-gnu/release/dxca "$STAGE/dxca"
cp deploy/dxca.service "$STAGE/deploy/"
cp install.sh "$STAGE/"

if [ "$NO_SEED" -eq 1 ]; then
  # The macOS plist is named for this station and is meaningless on a Pi —
  # it only ships with the seeded (own-shack) path.
  echo "--no-seed: shipping binary + service unit + installer only."
else
  cp deploy/com.vu2cpl.dxca.plist "$STAGE/deploy/" 2>/dev/null || true
  [ -f config/dxca.toml ] && cp config/dxca.toml "$STAGE/config/"
  for f in cty.xml lotw-users.txt dxca.db; do
    [ -f "data/$f" ] && cp "data/$f" "$STAGE/data/"
  done
fi

# Print exactly what is about to leave this machine. The seeding rules above
# are easy to reason about wrongly; a manifest is not.
echo
echo "Staged for $HOST:"
(cd "$STAGE" && find . -type f | sed 's|^\./|  |' | sort)
echo

# `${HOST}` braced and the ellipsis kept ASCII on purpose. Written as
# `$HOST…` this died with `HOST?: unbound variable` under bash 3.2 / a
# non-UTF-8 locale: those shells treat the ellipsis's high bytes as name
# characters, so the variable being looked up was `HOST\xe2\x80\xa6`, not
# HOST. It ran fine under bash 5 + UTF-8, which is what hid it. No `$VAR`
# in a runtime string should be followed by a non-ASCII byte.
echo "Shipping to ${HOST}..."
ssh "$HOST" 'mkdir -p ~/dxca-deploy'
rsync -a --delete "$STAGE/" "$HOST:dxca-deploy/"
# -t so a remote sudo that wants a password can actually ask for it; without
# a TTY the installer dies at the first sudo on any host lacking NOPASSWD.
ssh -t "$HOST" 'cd ~/dxca-deploy && bash install.sh pi'
echo "Done. Check: ssh $HOST systemctl status dxca"
