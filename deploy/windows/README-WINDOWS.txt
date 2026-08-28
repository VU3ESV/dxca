================================================================
 DXCA for Windows  —  v@VERSION@
 FT8/FT4 + DX-cluster spot aggregator with a multi-user web GUI
================================================================

READ THIS BEFORE INSTALLING. Windows support is brand new and only
lightly tested. The limitations below are real, not boilerplate.


----------------------------------------------------------------
 1. WHAT THIS IS
----------------------------------------------------------------

DXCA takes spots from your WSJT-X / JTDX instances (UDP) and from DX
cluster telnet nodes, merges and de-duplicates them, and serves the
result to your logging software over a built-in telnet cluster server
and to UDP destinations. A web GUI with per-user accounts lets each
operator carry their own ClubLog log matrix, New-DXCC / Slot / Band /
Mode highlighting, and Telegram alerts over one shared spot stream.

Everything is in one file. dxca.exe embeds the dashboard; there is no
runtime to install, no Rust, no Node, no Visual C++ redistributable.


----------------------------------------------------------------
 2. STATUS — PLEASE READ
----------------------------------------------------------------

DXCA runs in production 24/7 on Linux (Raspberry Pi) and macOS. The
Windows build is NEW as of 2026-08-28 and has a much thinner record.

WHAT WAS ACTUALLY TESTED
  * Cross-compiled from macOS for x86-64 Windows (GNU/mingw ABI).
  * Run on ONE machine: Windows 10 22H2, build 19045, AMD64.
  * Verified working there:
      - web GUI serves and the dashboard renders
      - /api/status and /api/spots respond
      - telnet cluster server accepts connections and sends its banner
      - SQLite database is created and initialised
      - installs as a LOCAL SYSTEM scheduled task with a boot trigger
      - survives the installing session closing
      - firewall rule opens it to the LAN and it is reachable there

WHAT WAS *NOT* TESTED — assume these are unproven
  * Receiving real spots. No WSJT-X, JTDX or DX-cluster node has ever
    fed the Windows build. The UDP decoder path and the cluster telnet
    client are unexercised on this platform.
  * Graceful shutdown. The Windows shutdown path handles Ctrl-C only
    and has never been exercised; the service is stopped by termination.
  * Long-running stability. The longest Windows run so far is minutes.
  * Windows 11, Windows Server, 32-bit, and ARM64. None tried.
  * A native MSVC build. This binary is a mingw/GNU cross-build made on
    a Mac, not what Visual Studio would produce.

If you need something dependable today, run DXCA on a Raspberry Pi or a
Linux box and open the web GUI from your Windows machine's browser.
That configuration is the one in daily production use.


----------------------------------------------------------------
 3. SECURITY — THE ONE THAT MATTERS
----------------------------------------------------------------

YOUR CLUBLOG AND TELEGRAM CREDENTIALS ARE STORED IN PLAIN TEXT.

DXCA keeps per-user ClubLog app passwords and Telegram bot tokens in
data\dxca.db, unencrypted. On Linux and macOS that file is protected by
being mode 0600 — readable only by the service account. That protection
is a Unix-only code path. ON WINDOWS IT IS SKIPPED ENTIRELY, and the
file is left with whatever ACLs it inherits from the folder.

In practice: anyone who can read your user folder, and any process
running as you, can read those credentials. There is no encryption at
rest and no Windows equivalent (DPAPI or ACL hardening) implemented yet.

  * Do not install DXCA on a shared or multi-user Windows machine.
  * Do not put this folder in a backup or sync target you share.
  * Use a ClubLog APP PASSWORD, never your main ClubLog password.
  * A Telegram bot token can be revoked via @BotFather if exposed.

Two smaller ones:

FIRST-RUN SETUP IS UNAUTHENTICATED. Before an admin account exists,
whoever loads the web GUI first can create it. install-dxca.cmd is built
around this: it runs the server on loopback, waits for you to create the
account, and only then offers to open the firewall. If you set DXCA up
by hand, keep that order.

THE SERVICE RUNS AS LOCAL SYSTEM. That is what allows a password-free
start at boot. It also means the process is more privileged than it
needs to be. Install it in a folder you control.


----------------------------------------------------------------
 4. INSTALLING
----------------------------------------------------------------

  1. Unzip this folder somewhere permanent, e.g. C:\dxca
     (the installer runs the service from wherever you put it).
  2. RIGHT-CLICK install-dxca.cmd -> "Run as administrator".
  3. Your browser opens on http://127.0.0.1:7580/ — create the admin
     account there. The installer waits for you.
  4. Answer whether to expose DXCA to your LAN.
  5. Done. It now starts automatically at every boot.

Windows SmartScreen may warn that the file is unrecognised, and some
antivirus products flag unsigned Rust binaries. dxca.exe is NOT code
signed — there is no certificate for this project. If that is not
acceptable to you, build it from source instead; see the repository.

  Start   schtasks /run /tn dxca
  Stop    schtasks /end /tn dxca
  Log     run.log in the install folder
  Remove  RIGHT-CLICK uninstall-dxca.cmd -> "Run as administrator"

The uninstaller removes the scheduled task and the firewall rules. It
deliberately leaves config\ and data\ alone — delete the folder yourself
when you actually mean to discard your accounts and settings.


----------------------------------------------------------------
 5. PORTS
----------------------------------------------------------------

  7580/tcp   web GUI                      (firewall rule "dxca-webui")
  7575/tcp   telnet cluster for loggers   (firewall rule "dxca-telnet")
  2333/udp   decoder source, default "MSHV"
  2334/udp   decoder source, default "JTDX"
  2335/udp   decoder source, default "WSJTX"

Point WSJT-X / JTDX / MSHV UDP output at this PC on the matching port,
and point your logger (Logger32, N1MM+, Log4OM, DXKeeper...) at this
PC's address on port 7575. All of it is editable in the web GUI's
System tab, which applies changes live and rewrites config\dxca.toml.

Firewall rules are added for the PRIVATE network profile only. If
Windows has your shack network classified as Public, the ports will not
open — change the network to Private in Windows network settings.

7575 is the default for a reason: CW Skimmer Server uses 7300 and 7550,
so DXCA stays out of its way when both run on the same machine.


----------------------------------------------------------------
 6. CREDITS AND LICENCE
----------------------------------------------------------------

Original concept and reference implementation by Vinod VU3ESV (FT8
Cluster Aggregator). DXCA 1.x is the macOS rewrite; this is DXCA 2.0,
the cross-platform successor — rewritten and extended.

The DX-cluster telnet client engine and the web GUI's design system are
derived from the Meridian project — joint work by Basil Thomas W6BT,
Vinod VU3ESV, and Ram VU3RDD.

Released under the MIT Licence. See LICENSE.

73 — Manoj VU2CPL
