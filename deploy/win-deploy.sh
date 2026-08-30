#!/usr/bin/env bash
# Cross-compile on the Mac and update a Windows DXCA over SSH — the Windows
# counterpart of pi-deploy.sh.
#
#   deploy/win-deploy.sh [user@host]
#
#     user@host   default manoj@192.168.1.170 (the shack Windows box).
#
# This is an UPDATE, not an install. It replaces dxca.exe in %SystemDrive%\DXCA
# (C:\DXCA on every normal machine) and restarts the scheduled task; it never
# touches config\ or data\, and it never registers anything. A first install
# is still install-dxca.cmd from the release zip, which is what creates the
# task, the firewall rules and the admin account.
#
# The install directory is READ FROM THE BOX, not assumed — install-dxca.cmd
# uses %SystemDrive%, so a machine whose Windows lives on D: keeps DXCA in
# D:\DXCA and this script follows it there.
#
# WHY THERE IS NO SEEDING HERE, unlike pi-deploy.sh: there is nothing this
# script could usefully seed. The Windows box has its own account, its own
# ClubLog credentials and its own database, and data\dxca.db carries all of
# them in plain text (README §Secrets). Shipping this station's copy over
# would hand them to whoever runs that machine and point a second host at the
# cluster nodes under the same callsign.
#
# REQUIREMENTS on the Windows side:
#   * OpenSSH Server running, with this Mac's key trusted.
#   * The SSH user in BUILTIN\Administrators, group ENABLED — the session
#     needs a full admin token to control the task and write to the
#     install directory.
#     Check with: whoami /groups | findstr S-1-5-32-544
#   * DXCA already installed at %SystemDrive%\DXCA by install-dxca.cmd.
#   * The SSH DefaultShell left as cmd.exe (the Windows default). Every
#     remote command below is cmd syntax — `if not exist`, `move /y`, `>nul`,
#     `&` — none of which PowerShell understands. Setting the registry's
#     OpenSSH\DefaultShell to PowerShell breaks this script, so the check
#     below refuses to run rather than failing halfway through a swap.
set -euo pipefail

HOST="${1:-manoj@192.168.1.170}"
WEBPORT=7580
# Strip user@ for the HTTP check — that runs from here, over the LAN.
HOSTNAME_ONLY="${HOST#*@}"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

SSH="ssh -o BatchMode=yes $HOST"

echo "Checking $HOST..."
# Fail early and by name. Every one of these is a precondition the operator
# can fix, and discovering them after the service is stopped would leave the
# box down for no reason.
#
# The shell check goes FIRST because every other command here — including the
# %SystemDrive% probe immediately below — is cmd syntax and would return
# nonsense rather than fail under PowerShell.
#
# %COMSPEC% expands only in cmd; PowerShell echoes it back literally. A
# cheap, exact test for the one thing every command below depends on.
if [ "$($SSH 'echo %COMSPEC%' 2>/dev/null | tr -d '\r')" = "%COMSPEC%" ]; then
  echo "win-deploy: $HOST's SSH shell is PowerShell, not cmd.exe." >&2
  echo "            Every remote command here is cmd syntax. Either reset" >&2
  echo "            HKLM:\\SOFTWARE\\OpenSSH\\DefaultShell to cmd.exe (or" >&2
  echo "            delete the value), or update this script." >&2
  exit 1
fi

# WHERE THE INSTALL IS. `install-dxca.cmd` sets
# `INSTALLDIR=%SystemDrive%\DXCA` — the drive Windows booted from, not the
# literal C:. This script hardcoded `C:\DXCA`, which is right on every box in
# this shack and wrong on any machine whose Windows lives elsewhere: it would
# report "no dxca.exe in C:\DXCA" — telling the operator there is no install
# when there is one, on D:. Ask the box instead of assuming, so the two
# scripts cannot disagree about where DXCA lives.
SYSTEM_DRIVE="$($SSH 'echo %SystemDrive%' 2>/dev/null | tr -d '\r')"
case "$SYSTEM_DRIVE" in
  [A-Za-z]:) ;;
  # Empty, or echoed back unexpanded. Fall back rather than build a path out
  # of a garbage string — C: is right on every machine seen so far, and the
  # dxca.exe check below turns a wrong guess into a clear error, not damage.
  *) SYSTEM_DRIVE='C:' ;;
esac
INSTALLDIR="${SYSTEM_DRIVE}\\DXCA"
# The same path with forward slashes, for scp. Modern scp speaks SFTP, whose
# path syntax is POSIX-ish — a backslash there is an escape character, not a
# separator, so the Windows form silently addresses the wrong file.
INSTALLDIR_SFTP="${SYSTEM_DRIVE}/DXCA"
echo "Install directory: $INSTALLDIR"

$SSH "if not exist \"$INSTALLDIR\\dxca.exe\" exit 1" || {
  echo "win-deploy: no dxca.exe in $INSTALLDIR on $HOST." >&2
  echo "            This updates an existing install; run install-dxca.cmd" >&2
  echo "            from the release zip for a first install." >&2
  exit 1
}
$SSH 'whoami /groups | findstr /c:"S-1-5-32-544" >nul' || {
  echo "win-deploy: $HOST's SSH user is not an enabled Administrator." >&2
  echo "            It cannot stop the task or write to $INSTALLDIR." >&2
  exit 1
}

echo "Building web UI + Windows binary..."
pnpm -C web-ui install && pnpm -C web-ui build
cargo zigbuild --release -p dxca-server --target x86_64-pc-windows-gnu
EXE=target/x86_64-pc-windows-gnu/release/dxca.exe
[ -f "$EXE" ] || { echo "win-deploy: $EXE not built." >&2; exit 1; }

# Upload BEFORE stopping anything. A transfer that fails then costs nothing
# but time — the running install is untouched and still serving.
echo "Uploading $(du -h "$EXE" | cut -f1) to $INSTALLDIR\\dxca.exe.new ..."
scp -q "$EXE" "$HOST:$INSTALLDIR_SFTP/dxca.exe.new"

echo "Stopping the service..."
# taskkill as well as schtasks /end: the task ending does not guarantee the
# process is gone, and Windows will not let a running exe be replaced.
$SSH "schtasks /end /tn dxca >nul 2>&1 & taskkill /im dxca.exe /f >nul 2>&1 & exit 0"

# Poll rather than sleep a fixed time. A just-stopped process does not
# release its listener instantly, and a fixed guess is either too slow every
# time or too short on the one run that matters.
echo "Waiting for port $WEBPORT to be released..."
for _ in $(seq 20); do
  $SSH "netstat -ano | findstr /R /C:\":$WEBPORT .*LISTENING\" >nul" || break
  sleep 0.5
done

echo "Swapping the binary..."
# Keep the outgoing exe as .bak. If the new one will not start, that file is
# the whole of the rollback, and it costs 11 MB of disk to have it.
$SSH "move /y \"$INSTALLDIR\\dxca.exe\" \"$INSTALLDIR\\dxca.exe.bak\" >nul && move /y \"$INSTALLDIR\\dxca.exe.new\" \"$INSTALLDIR\\dxca.exe\" >nul"

echo "Starting the service..."
$SSH "schtasks /run /tn dxca >nul"

echo "Checking http://$HOSTNAME_ONLY:$WEBPORT/ ..."
UP=""
for _ in $(seq 20); do
  if curl -s -m 3 -o /dev/null "http://$HOSTNAME_ONLY:$WEBPORT/"; then UP=1; break; fi
  sleep 1
done

if [ -z "$UP" ]; then
  echo >&2
  echo "win-deploy: the task was started but the dashboard is not answering." >&2
  echo "            Rolling back to the previous binary." >&2
  $SSH "schtasks /end /tn dxca >nul 2>&1 & taskkill /im dxca.exe /f >nul 2>&1 & exit 0"
  $SSH "move /y \"$INSTALLDIR\\dxca.exe.bak\" \"$INSTALLDIR\\dxca.exe\" >nul"
  $SSH "schtasks /run /tn dxca >nul"
  echo "            Rolled back. Read $INSTALLDIR\\run.log on $HOST." >&2
  exit 1
fi

echo
echo "OK: http://$HOSTNAME_ONLY:$WEBPORT/ is serving the dashboard."
echo "    config\\ and data\\ were not touched."
echo "    Previous binary kept at $INSTALLDIR\\dxca.exe.bak"
