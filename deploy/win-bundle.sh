#!/usr/bin/env bash
# Build the shippable Windows bundle: one .exe, two .cmd scripts, the
# disclaimers, and the licence — zipped, ready to hand to someone.
#
#   deploy/win-bundle.sh [--out DIR]
#
# WHY THIS CROSS-COMPILES RATHER THAN BUILDING ON WINDOWS.
# A native Windows build needs the MSVC Build Tools, because `ring` and
# bundled SQLite both compile C. Cross-compiling with cargo-zigbuild uses
# zig's bundled mingw-w64 headers instead, so the bundle can be produced on
# the Mac that already builds the Pi target the same way (Justfile `dist`).
#
# The result is a GNU-ABI binary, NOT what Visual Studio would emit. That is
# stated plainly in README-WINDOWS.txt and must stay stated — it is the
# single biggest caveat on the artifact.
#
# Needs: rustup target x86_64-pc-windows-gnu, cargo-zigbuild, zig, pnpm.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

OUT="$REPO/target/win-bundle"
for arg in "$@"; do
  case "$arg" in
    --out) shift; OUT="${1:-$OUT}" ;;
    -h|--help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  esac
  shift || true
done

TARGET=x86_64-pc-windows-gnu
VERSION="$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)"
STAGE="$OUT/dxca-$VERSION-windows-x64"

command -v cargo-zigbuild >/dev/null 2>&1 || {
  echo "win-bundle: cargo-zigbuild not found (brew install cargo-zigbuild zig)" >&2
  exit 1
}
rustup target list --installed | grep -qx "$TARGET" || {
  echo "win-bundle: missing rust target — run: rustup target add $TARGET" >&2
  exit 1
}

# The dashboard is embedded at COMPILE time, so dist must exist FIRST or the
# binary silently ships build.rs's placeholder page. Same trap install.sh
# documents for the Pi.
echo "==> Building the dashboard..."
pnpm -C web-ui install --silent
pnpm -C web-ui build

echo "==> Cross-compiling for $TARGET..."
rustup run stable cargo zigbuild --release -p dxca-server --target "$TARGET"

BIN="$REPO/target/$TARGET/release/dxca.exe"
[ -f "$BIN" ] || { echo "win-bundle: $BIN was not produced" >&2; exit 1; }

# Refuse to ship a binary serving the placeholder — the failure that looks
# exactly like success until someone opens the page.
if strings -a "$BIN" | grep -q "Web UI not built into this binary"; then
  echo "win-bundle: the binary carries the PLACEHOLDER page, not the dashboard." >&2
  echo "            web-ui/dist was missing or stale when cargo ran." >&2
  exit 1
fi

echo "==> Assembling $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$BIN"                                   "$STAGE/dxca.exe"
cp "$REPO/deploy/windows/install-dxca.cmd"   "$STAGE/"
cp "$REPO/deploy/windows/uninstall-dxca.cmd" "$STAGE/"
cp "$REPO/deploy/windows/README-WINDOWS.txt" "$STAGE/"
cp "$REPO/LICENSE"                           "$STAGE/LICENSE.txt"

# CRLF for the files a Windows user opens in Notepad or runs as a batch.
# A .cmd with bare LF endings still runs, but mangles multi-line prompts on
# older shells — not worth the risk on an artifact handed to strangers.
for f in "$STAGE"/*.cmd "$STAGE"/*.txt; do
  perl -pi -e 's/\r?\n/\r\n/' "$f"
done

ZIP="$OUT/dxca-$VERSION-windows-x64.zip"
rm -f "$ZIP"
( cd "$OUT" && zip -qr "$(basename "$ZIP")" "$(basename "$STAGE")" )

echo
echo "Bundle : $ZIP"
echo "Staged : $STAGE"
ls -la "$STAGE"
echo
echo "NOTE: dxca.exe is UNSIGNED. SmartScreen will warn on first run."
