#!/usr/bin/env bash
# Cross-compile on the Mac and install dxca on a Docker host as a container —
# the sibling of pi-deploy.sh for boxes that already run their services in
# Docker (192.168.1.109, `ubersdr`).
#
#   deploy/docker-deploy.sh [user@host]
#
#     user@host   default vu2cpl@192.168.1.109.
#
# What it does:
#   1. builds the web UI and a Linux binary for the host's architecture
#      (`uname -m` over ssh: x86_64 or aarch64);
#   2. ships that binary + deploy/Dockerfile and runs `docker build` THERE,
#      tagging dxca:<version> and dxca:latest — no registry, and no Docker
#      needed on the Mac;
#   3. smoke-tests the image in a throwaway container with --network none and
#      empty config/data, so the test can reach nothing and log in nowhere;
#   4. (re)creates the container `dxca`, bind-mounting the host's
#      /opt/dxca/config and /opt/dxca/data. A `dxca` container that was
#      RUNNING is started again on the new image. Otherwise it is left
#      CREATED, not started.
#
# NEVER SHIPS CONFIG OR DATA — the same rule as `pi-deploy.sh --no-seed`, for
# the same reasons: data/dxca.db holds ClubLog passwords, API keys and the
# Telegram token in plain text, and config/dxca.toml holds this station's
# cluster logins, which a second running copy would fight over (DXSpider kicks
# duplicate logins). Copying another host's state in is a separate, deliberate
# step.
#
# A never-started container stays stopped across reboots: Docker applies the
# restart policy only once a container has been started. So a staged
# container cannot come up by itself and send a second copy of every alert.
#
# HOST NETWORKING, NOT PORT MAPPING. UDP source and destination ports are
# edited at runtime in the web UI; a port mapping fixed at `docker create`
# time would silently strand any source added later. Host networking also
# keeps the container's view of the LAN identical to a native install's.
#
# Needs on the host: Docker, and passwordless sudo (for /opt/dxca, and for
# docker itself unless the ssh user is in the `docker` group).
# Needs on the Mac: cargo-zigbuild, zig, pnpm, and the rustup target for the
# host — `rustup target add x86_64-unknown-linux-gnu` for an x86-64 box.
set -euo pipefail

HOST="${1:-vu2cpl@192.168.1.109}"
NAME=dxca

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"

ARCH="$(ssh "$HOST" uname -m)"
case "$ARCH" in
  x86_64)  TARGET=x86_64-unknown-linux-gnu ;;
  aarch64) TARGET=aarch64-unknown-linux-gnu ;;
  *) echo "docker-deploy: unsupported architecture '$ARCH' on $HOST" >&2; exit 1 ;;
esac

echo "Building web UI + $TARGET binary (dxca $VERSION)..."
pnpm -C web-ui install && pnpm -C web-ui build
# glibc 2.36 floor, like the Pi target. The image's Debian trixie has 2.41.
cargo zigbuild --release -p dxca-server --target "$TARGET.2.36"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp "target/$TARGET/release/dxca" "$STAGE/dxca"
cp deploy/Dockerfile "$STAGE/Dockerfile"

echo "Shipping to ${HOST}..."
ssh "$HOST" 'mkdir -p ~/dxca-docker'
rsync -a --delete "$STAGE/" "$HOST:dxca-docker/"

ssh "$HOST" "NAME=$NAME VERSION=$VERSION bash -s" <<'REMOTE'
set -euo pipefail
DOCKER=docker
docker info >/dev/null 2>&1 || DOCKER="sudo -n docker"
UIDGID="$(id -u):$(id -g)"

echo "Building image dxca:$VERSION..."
$DOCKER build -q -t "dxca:$VERSION" -t dxca:latest ~/dxca-docker >/dev/null

# Smoke test: no network at all, empty state, removed afterwards.
SMOKE="$(mktemp -d)"
mkdir -p "$SMOKE/config" "$SMOKE/data"
$DOCKER run -d --rm --name dxca-smoke --network none --user "$UIDGID" \
  -v "$SMOKE/config:/opt/dxca/config" -v "$SMOKE/data:/opt/dxca/data" \
  "dxca:$VERSION" >/dev/null
STATUS=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  sleep 1
  STATUS="$($DOCKER exec dxca-smoke bash -c \
    'exec 3<>/dev/tcp/127.0.0.1/7580 && printf "GET /api/status HTTP/1.0\r\n\r\n" >&3 && cat <&3' \
    2>/dev/null | tail -1)" && [ -n "$STATUS" ] && break
done
$DOCKER rm -f dxca-smoke >/dev/null 2>&1 || true
rm -rf "$SMOKE"
case "$STATUS" in
  *"\"version\":\"$VERSION\""*) echo "Smoke test OK: /api/status reports $VERSION." ;;
  *) echo "Smoke test FAILED — /api/status said: ${STATUS:-nothing}" >&2; exit 1 ;;
esac

sudo -n mkdir -p /opt/dxca/config /opt/dxca/data
sudo -n chown "$UIDGID" /opt/dxca /opt/dxca/config /opt/dxca/data

WAS_RUNNING=0
if $DOCKER container inspect "$NAME" >/dev/null 2>&1; then
  if [ "$($DOCKER container inspect -f '{{.State.Running}}' "$NAME")" = true ]; then
    WAS_RUNNING=1
    $DOCKER stop "$NAME" >/dev/null
  fi
  $DOCKER rm "$NAME" >/dev/null
fi
$DOCKER create --name "$NAME" \
  --network host \
  --restart unless-stopped \
  --user "$UIDGID" \
  -v /opt/dxca/config:/opt/dxca/config \
  -v /opt/dxca/data:/opt/dxca/data \
  --log-opt max-size=10m --log-opt max-file=3 \
  "dxca:$VERSION" >/dev/null

if [ "$WAS_RUNNING" = 1 ]; then
  $DOCKER start "$NAME" >/dev/null
  echo "dxca $VERSION: container was running, restarted on the new image."
else
  echo "dxca $VERSION: container created, NOT started. Start: $DOCKER start $NAME"
fi
$DOCKER ps -a --filter "name=^${NAME}\$" --format '{{.Names}}  {{.Image}}  {{.Status}}'
REMOTE
