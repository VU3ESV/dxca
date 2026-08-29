@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0"

rem ---------------------------------------------------------------------
rem DXCA Windows installer.
rem
rem Installs dxca.exe (in this folder) as a LOCAL SYSTEM scheduled task that
rem starts at boot, and optionally opens the Windows Firewall so the rest of
rem the LAN can reach the web GUI and the telnet cluster.
rem
rem Run this by RIGHT-CLICKING it and choosing "Run as administrator".
rem
rem Order matters and this script enforces it: the first-run setup card is
rem UNAUTHENTICATED, so the admin account is created over loopback FIRST and
rem the firewall is only opened once an account exists. Opening the port
rem first would let anyone on the LAN claim the admin account.
rem
rem Read README-WINDOWS.txt before running. Windows support is new and
rem lightly tested — the disclaimers there are real.
rem ---------------------------------------------------------------------

set "TASKNAME=dxca"
set "WEBPORT=7580"
set "TELNETPORT=7575"
rem WHERE THE ZIP WAS UNPACKED versus WHERE DXCA LIVES — two different
rem places, and conflating them was the bug this fixes.
rem
rem The installer used to run DXCA from whatever folder it was unzipped
rem into. Every release unzips as dxca-<version>-windows-x64, so each
rem upgrade landed in a NEW folder with no config and no database: a fresh
rem install every time, with the account, ClubLog credentials, log matrix
rem and alert history stranded in the previous version's folder.
rem
rem DXCA now lives in one fixed place and the unzipped folder is only a
rem delivery vehicle. C:\DXCA: machine-wide rather than tied to whichever
rem account ran the installer, short enough to type into a support
rem question, and visible in Explorer without unhiding anything.
rem SystemDrive rather than a literal C: for the rare machine that boots
rem from another letter; it reads as C:\DXCA everywhere else.
set "SRCDIR=%~dp0"
if "%SRCDIR:~-1%"=="\" set "SRCDIR=%SRCDIR:~0,-1%"
set "INSTALLDIR=%SystemDrive%\DXCA"

echo.
echo ==========================================================
echo  DXCA for Windows — installer
echo ==========================================================
echo  From   : %SRCDIR%
echo  Install: %INSTALLDIR%
echo  Web GUI: port %WEBPORT%
echo  Telnet : port %TELNETPORT%
echo.

rem --- must be elevated ------------------------------------------------
rem S-1-16-12288 is the High Mandatory Level SID. `net session` is NOT a
rem reliable elevation probe here — it fails for unrelated reasons on a box
rem with the Server service stopped.
whoami /groups | findstr /C:"S-1-16-12288" >nul
if errorlevel 1 (
  echo [X] This installer must run elevated.
  echo     Right-click install-dxca.cmd and choose "Run as administrator".
  echo.
  pause
  exit /b 1
)
echo [ok] Running elevated.

rem --- the binary must be here -----------------------------------------
if not exist "%SRCDIR%\dxca.exe" (
  echo [X] dxca.exe not found in this folder.
  echo     Keep install-dxca.cmd next to dxca.exe and re-run.
  echo.
  pause
  exit /b 1
)
echo [ok] Found dxca.exe.

rem --- curl is needed for the readiness checks -------------------------
where curl.exe >nul 2>&1
if errorlevel 1 (
  echo [X] curl.exe not found. It ships with Windows 10 1803 and later.
  echo     This installer needs it to verify the server came up.
  echo.
  pause
  exit /b 1
)

rem --- create the install folder, with permissions that hold ------------
if not exist "%INSTALLDIR%" (
  mkdir "%INSTALLDIR%"
  rem It is made HERE, ahead of the import block, because that block does
  rem `mkdir %INSTALLDIR%\config` and cmd creates intermediate folders on
  rem the way — so doing this later would find the folder already present
  rem and skip the lockdown on exactly the installs that need it.
  rem
  rem A new folder at the root of the system drive inherits the drive
  rem root's permissions, and those let ANY standard user write inside it.
  rem dxca.exe is started by a LOCAL SYSTEM task, so a writable exe is a
  rem path from ordinary user to SYSTEM — and data\dxca.db holds ClubLog
  rem and Telegram credentials in plain text. Both want closing.
  rem
  rem Well-known SIDs, not group names: "Administrators" and "Users" are
  rem translated on a localised Windows and icacls would not match them.
  rem   S-1-5-32-544 Administrators, S-1-5-18 SYSTEM, S-1-5-32-545 Users
  icacls "%INSTALLDIR%" /inheritance:r /grant:r "*S-1-5-32-544:(OI)(CI)F" /grant:r "*S-1-5-18:(OI)(CI)F" /grant:r "*S-1-5-32-545:(OI)(CI)RX" >nul 2>&1
  rem A warning, never a failure: a working install matters more, and the
  rem operator can see and fix this afterwards.
  if errorlevel 1 (
    echo [--] Could not tighten permissions on %INSTALLDIR%.
    echo      It works, but standard users on this PC can read and change
    echo      the files there — including credentials in data\dxca.db.
  ) else (
    echo [ok] %INSTALLDIR% created ^(administrators only^).
  )
)

rem --- fresh install, or an update over an existing one? -----------------
rem An existing config + database in the install folder means this is a
rem live install being updated. That distinction decides everything below:
rem an update MUST NOT rewrite config\dxca.toml, because by then it holds the operator's UDP
rem sources, cluster nodes and broadcast destinations as edited in the web
rem GUI's System tab. Overwriting it would silently discard the whole
rem station setup and look like a successful upgrade.
set "UPGRADE="
if exist "%INSTALLDIR%\config\dxca.toml" (
  if exist "%INSTALLDIR%\data\dxca.db" set "UPGRADE=1"
)

if defined UPGRADE (
  echo [ok] Existing install found in %INSTALLDIR% — updating it.
  echo      config\dxca.toml and data\ will NOT be touched.
) else (
  echo [ok] Nothing installed in %INSTALLDIR% yet.
)

rem --- carry a previous install's settings across -------------------------
rem This runs ONCE, for the machine that is moving off the old layout.
rem
rem Installs made before the fixed location ran DXCA from the unzipped
rem folder, so their config and database sit in whichever version-named
rem folder the operator last unzipped. Left alone they would be orphaned
rem there and this would look like a fresh install — a new empty database,
rem and the account, ClubLog credentials, log matrix and alert history all
rem stranded. That is the "every install needs reconfiguring" complaint,
rem and it is data loss rather than mere inconvenience.
rem
rem After this has run once the station lives in %INSTALLDIR% and
rem every later upgrade is detected as one — this block is skipped entirely
rem and nothing is ever asked again.
rem
rem The reliable mechanism is the operator naming the old folder; detecting
rem it from the existing scheduled task is only a convenience, because the
rem task listing is English-only and its encoding varies by Windows build.
rem Detection failing must never block the import.
if not defined UPGRADE (
  set "OLDDIR="
  rem The line reads:  Task To Run:   C:\somewhere\run-dxca.cmd
  rem
  rem `tokens=2*` was WRONG here and silently so: token 2 is "To", and the
  rem `*` remainder therefore begins at "Run:", giving
  rem "Run:            C:\somewhere\run-dxca.cmd" — a string %%~dpp cannot
  rem make a folder out of. Detection never once succeeded, and because a
  rem failure here is designed to fall through to a prompt rather than
  rem complain, nothing said so. Found on Manoj's first real Windows test
  rem (2026-08-29): the installer offered no previous install and he copied
  rem config\ and data\ into C:\DXCA by hand.
  rem
  rem Splitting on ":" is not the fix either — the path has one in it.
  rem `!VAR:*needle=!` deletes everything up to and including the needle,
  rem which leaves the path whatever it contains.
  set "OLDLINE="
  for /f "tokens=*" %%a in (
    'schtasks /query /tn "%TASKNAME%" /v /fo list 2^>nul ^| findstr /i /c:"Task To Run:"'
  ) do set "OLDLINE=%%a"
  set "OLDEXE="
  if defined OLDLINE (
    set "OLDEXE=!OLDLINE:*Task To Run:=!"
    rem schtasks pads the value out with spaces; strip them.
    for /f "tokens=* delims= " %%x in ("!OLDEXE!") do set "OLDEXE=%%x"
  )
  if defined OLDEXE (
    for %%p in ("!OLDEXE!") do set "OLDDIR=%%~dpp"
    if "!OLDDIR:~-1!"=="\" set "OLDDIR=!OLDDIR:~0,-1!"
  )

  rem A detected folder only counts if it really holds an install and is not
  rem this one.
  set "SUGGEST="
  if defined OLDDIR (
    if /i not "!OLDDIR!"=="%INSTALLDIR%" (
      if exist "!OLDDIR!\config\dxca.toml" if exist "!OLDDIR!\data\dxca.db" set "SUGGEST=!OLDDIR!"
    )
  )

  rem Second look: the unzipped folder itself. Before this release DXCA
  rem ran from there, so an operator who unzipped the new version over
  rem their old install has the whole station sitting right beside it.
  if not defined SUGGEST (
    if exist "%SRCDIR%\config\dxca.toml" if exist "%SRCDIR%\data\dxca.db" set "SUGGEST=%SRCDIR%"
  )

  rem Third look, and in practice the one that fires: a SIBLING folder.
  rem Each release unzips as dxca-<version>-windows-x64, and the old
  rem instructions had operators keep them side by side under one parent
  rem (C:\dxca\dxca-2.8.0-... next to C:\dxca\dxca-2.9.0-...). Unlike
  rem reading the scheduled task, this works on any language of Windows
  rem and needs nothing running. Newest first; the first one holding a
  rem real install wins.
  if not defined SUGGEST (
    for /f "delims=" %%d in ('dir /b /ad /o-d "%SRCDIR%\..\dxca-*" 2^>nul') do (
      if not defined SUGGEST (
        set "CAND=%SRCDIR%\..\%%d"
        for %%f in ("!CAND!") do set "CAND=%%~ff"
        if /i not "!CAND!"=="%INSTALLDIR%" (
          if exist "!CAND!\config\dxca.toml" if exist "!CAND!\data\dxca.db" set "SUGGEST=!CAND!"
        )
      )
    )
  )

  echo.
  echo  DXCA will now live in %INSTALLDIR%, and stay there — future
  echo  upgrades keep your settings automatically, with nothing to answer.
  echo  If you ran DXCA before, its config\ and data\ are still in the
  echo  folder you unzipped back then and can be carried over now.
  if defined SUGGEST echo  Found a previous install: !SUGGEST!
  echo.
  if defined SUGGEST (
    set /p "IMPORT=Import settings from that folder? [Y/n, or type another path] "
    if "!IMPORT!"=="" set "IMPORT=Y"
    if /i "!IMPORT!"=="Y" set "IMPORT=!SUGGEST!"
  ) else (
    set /p "IMPORT=Path of your previous DXCA folder, or blank if this is your first install: "
  )

  if /i "!IMPORT!"=="n" set "IMPORT="
  if defined IMPORT (
    rem Trim quotes an operator pasted or Explorer added. The unquoted
    rem SET form is deliberate: `set "X=!X:"=!"` trips the parser inside a
    rem parenthesised block.
    set IMPORT=!IMPORT:"=!
    if "!IMPORT:~-1!"=="\" set "IMPORT=!IMPORT:~0,-1!"
    if not exist "!IMPORT!\data\dxca.db" (
      echo.
      echo *** No data\dxca.db in "!IMPORT!" — nothing imported.
      echo     Continuing as a fresh install.
      echo.
      set "IMPORT="
    )
  )

  if defined IMPORT (
    if not exist "%INSTALLDIR%\config" mkdir "%INSTALLDIR%\config"
    if not exist "%INSTALLDIR%\data"   mkdir "%INSTALLDIR%\data"
    rem The database first: it carries the accounts, so a half-done import
    rem that got the config but not the database would be the worst outcome.
    copy /y "!IMPORT!\data\dxca.db" "%INSTALLDIR%\data\dxca.db" >nul
    if errorlevel 1 (
      echo *** Could not copy the database. Is DXCA still running from
      echo     "!IMPORT!"? Close it and run this installer again.
      pause
      exit /b 1
    )
    rem cty.xml and the LoTW list are large downloads; carrying them saves
    rem the first run fetching ~16 MB again. Absent ones are not an error.
    if exist "!IMPORT!\data\cty.xml" copy /y "!IMPORT!\data\cty.xml" "%INSTALLDIR%\data\" >nul
    if exist "!IMPORT!\data\lotw-users.txt" copy /y "!IMPORT!\data\lotw-users.txt" "%INSTALLDIR%\data\" >nul
    if exist "!IMPORT!\config\dxca.toml" copy /y "!IMPORT!\config\dxca.toml" "%INSTALLDIR%\config\dxca.toml" >nul
    echo [ok] Imported settings and database from "!IMPORT!".
    echo      Your account, ClubLog credentials and log matrix came with them.
    rem From here this behaves exactly like an in-place update: the config is
    rem now the operator's, and must not be rewritten below.
    set "UPGRADE=1"
  ) else (
    echo [ok] Fresh install.
  )
)

rem --- stop whatever is running before touching the binary ---------------
schtasks /query /tn "%TASKNAME%" >nul 2>&1
if not errorlevel 1 (
  if not defined UPGRADE (
    echo.
    echo *** A scheduled task named "%TASKNAME%" already exists, but this
    echo     folder has no config/database — so this is not an update.
    echo.
    set /p "GOON=Replace the existing task? [y/N] "
    if /i not "!GOON!"=="y" (
      echo Aborted — nothing changed.
      pause
      exit /b 1
    )
  )
  schtasks /end /tn "%TASKNAME%" >nul 2>&1
  schtasks /delete /tn "%TASKNAME%" /f >nul 2>&1
)
taskkill /im dxca.exe /f >nul 2>&1

rem --- anything still holding our ports? ---------------------------------
rem Poll: a just-stopped process does not release its listener instantly, and
rem failing on the first check would abort every legitimate update.
set "PORTBUSY=1"
for /l %%i in (1,1,10) do (
  if defined PORTBUSY (
    netstat -ano | findstr /R /C:":%WEBPORT% .*LISTENING" >nul || set "PORTBUSY="
    if defined PORTBUSY powershell -NoProfile -Command "Start-Sleep -Milliseconds 500" >nul
  )
)
if defined PORTBUSY (
  echo [X] Something is still listening on port %WEBPORT%.
  echo     Stop it first — two aggregators cannot share a port.
  echo.
  pause
  exit /b 1
)

if not exist "%INSTALLDIR%\config" mkdir "%INSTALLDIR%\config"
if not exist "%INSTALLDIR%\data"   mkdir "%INSTALLDIR%\data"

rem --- put this release's binary in the install folder --------------------
rem After the task is stopped and the port is free, never before: copying
rem over a running exe fails, and failing here after the service is down
rem would leave the machine with no DXCA at all.
copy /y "%SRCDIR%\dxca.exe" "%INSTALLDIR%\dxca.exe" >nul
if errorlevel 1 (
  echo [X] Could not copy dxca.exe into %INSTALLDIR%.
  echo     Is DXCA still running, or is this folder write-protected?
  echo.
  pause
  exit /b 1
)
echo [ok] Installed dxca.exe to %INSTALLDIR%.

rem An update replaces the binary and re-registers the task, nothing else.
rem The account already exists, so there is no setup card to protect, and
rem the config is the operator's — so both phases below are skipped whole.
if defined UPGRADE goto :register

rem =====================================================================
rem  PHASE 1 — loopback only, create the admin account
rem =====================================================================
echo.
echo --- Phase 1: first run on loopback -----------------------------------
echo Writing a loopback-only config so nothing on the LAN can reach the
echo setup card before you have claimed the admin account.

> "%INSTALLDIR%\config\dxca.toml" echo # DXCA configuration. The web GUI's System tab rewrites this file.
>>"%INSTALLDIR%\config\dxca.toml" echo web_bind = "127.0.0.1:%WEBPORT%"
>>"%INSTALLDIR%\config\dxca.toml" echo telnet_port = %TELNETPORT%
>>"%INSTALLDIR%\config\dxca.toml" echo data_dir = "data"

echo Starting dxca temporarily...
rem Relative names on purpose: we cd'd to the install folder at the top, so
rem no path needs quoting here. Batch has no \" escape — spelling this with
rem backslash-escaped quotes silently produces a broken path.
start "" /b /d "%INSTALLDIR%" cmd /c "dxca.exe > setup.log 2>&1"

rem Poll rather than sleep a fixed time — a cold start varies.
set "UP="
for /l %%i in (1,1,20) do (
  if not defined UP (
    powershell -NoProfile -Command "Start-Sleep -Milliseconds 700" >nul
    curl.exe -s -m 3 -o nul http://127.0.0.1:%WEBPORT%/ && set "UP=1"
  )
)
if not defined UP (
  echo [X] dxca did not start. Log:
  if exist "%INSTALLDIR%\setup.log" type "%INSTALLDIR%\setup.log"
  taskkill /im dxca.exe /f >nul 2>&1
  echo.
  pause
  exit /b 1
)
echo [ok] Server is up on http://127.0.0.1:%WEBPORT%/

rem Already set up from a previous attempt?
powershell -NoProfile -Command "try { if ((Invoke-RestMethod -TimeoutSec 5 http://127.0.0.1:%WEBPORT%/api/status).setup_required) { exit 1 } else { exit 0 } } catch { exit 2 }"
if errorlevel 1 (
  echo.
  echo Opening your browser. Create the admin account there:
  echo     http://127.0.0.1:%WEBPORT%/
  echo.
  start "" "http://127.0.0.1:%WEBPORT%/"
  echo Waiting for you to create the account...
  set "DONE="
  for /l %%i in (1,1,600) do (
    if not defined DONE (
      powershell -NoProfile -Command "Start-Sleep -Seconds 1" >nul
      powershell -NoProfile -Command "try { if ((Invoke-RestMethod -TimeoutSec 3 http://127.0.0.1:%WEBPORT%/api/status).setup_required) { exit 1 } else { exit 0 } } catch { exit 2 }"
      if not errorlevel 1 set "DONE=1"
    )
  )
  if not defined DONE (
    echo.
    echo [X] No admin account was created within 10 minutes.
    echo     Nothing has been exposed to the LAN. Re-run when ready.
    taskkill /im dxca.exe /f >nul 2>&1
    echo.
    pause
    exit /b 1
  )
)
echo [ok] Admin account exists.

taskkill /im dxca.exe /f >nul 2>&1
powershell -NoProfile -Command "Start-Sleep -Seconds 2" >nul

rem =====================================================================
rem  PHASE 2 — LAN exposure (optional), firewall, service
rem =====================================================================
echo.
echo --- Phase 2: service installation ------------------------------------
echo.
echo Reach the web GUI and telnet cluster from OTHER machines on your LAN?
echo Answering N keeps DXCA loopback-only — usable on this PC only, and no
echo firewall rules are added.
set /p "LAN=Expose to the LAN? [Y/n] "
if /i "%LAN%"=="n" (set "EXPOSE=0") else (set "EXPOSE=1")

if "%EXPOSE%"=="1" (
  > "%INSTALLDIR%\config\dxca.toml" echo # DXCA configuration. The web GUI's System tab rewrites this file.
  >>"%INSTALLDIR%\config\dxca.toml" echo web_bind = "0.0.0.0:%WEBPORT%"
  >>"%INSTALLDIR%\config\dxca.toml" echo telnet_port = %TELNETPORT%
  >>"%INSTALLDIR%\config\dxca.toml" echo data_dir = "data"

  rem Private profile only — a shack LAN, not a coffee-shop network.
  netsh advfirewall firewall delete rule name="dxca-webui"  >nul 2>&1
  netsh advfirewall firewall delete rule name="dxca-telnet" >nul 2>&1
  netsh advfirewall firewall add rule name="dxca-webui"  dir=in action=allow protocol=TCP localport=%WEBPORT%    profile=private description="DXCA web GUI" >nul
  netsh advfirewall firewall add rule name="dxca-telnet" dir=in action=allow protocol=TCP localport=%TELNETPORT% profile=private description="DXCA telnet cluster" >nul
  echo [ok] Firewall rules added ^(Private profile only^).

  rem The rules are scoped to the Private profile deliberately — a shack LAN,
  rem not a coffee-shop network. But Windows classifies an unrecognised
  rem network as PUBLIC by default, and re-classifies on its own if the
  rem adapter re-identifies the network. When that happens the rules are
  rem present, enabled, and completely inert: the install reports success and
  rem nothing outside this PC can connect. Check rather than assume.
  powershell -NoProfile -Command "if (Get-NetConnectionProfile | Where-Object { $_.NetworkCategory -ne 'Private' }) { exit 1 } else { exit 0 }"
  if errorlevel 1 (
    echo.
    echo  *** WARNING — your network is NOT classified as Private.
    echo.
    powershell -NoProfile -Command "Get-NetConnectionProfile | ForEach-Object { '      ' + $_.Name + ' [' + $_.InterfaceAlias + '] = ' + $_.NetworkCategory }"
    echo.
    echo      The firewall rules just added apply to the Private profile
    echo      only, so on a Public network they do nothing and DXCA will
    echo      NOT be reachable from other machines — even though it is
    echo      running and listening correctly.
    echo.
    echo      Setting a network to Private relaxes Windows' firewall for
    echo      this network generally, not just for DXCA. Only do it on a
    echo      network you trust, such as your own shack LAN.
    echo.
    set /p "NETFIX=Set your active network(s) to Private now? [y/N] "
    if /i "!NETFIX!"=="y" (
      powershell -NoProfile -Command "Get-NetConnectionProfile | Where-Object { $_.NetworkCategory -ne 'Private' } | Set-NetConnectionProfile -NetworkCategory Private"
      echo      [ok] Network set to Private.
      set "LANOK=1"
    ) else (
      echo      [--] Left unchanged. DXCA will work on this PC only.
      echo           To change it later: Settings ^> Network ^& Internet ^>
      echo           your connection ^> Network profile type ^> Private.
    )
  ) else (
    echo [ok] Network is classified Private — the rules will apply.
    set "LANOK=1"
  )
) else (
  echo [ok] Staying loopback-only. No firewall rules added.
)

:register
rem The service is launched through a tiny generated wrapper rather than a
rem long schtasks /tr string. Two reasons: /tr quoting breaks the moment the
rem install path contains a space, and the wrapper's `cd /d "%~dp0"` is what
rem makes the RELATIVE config\dxca.toml resolve. Without a working directory
rem dxca silently comes up on built-in defaults instead of your config.
> "%INSTALLDIR%\run-dxca.cmd" echo @echo off
>>"%INSTALLDIR%\run-dxca.cmd" echo cd /d "%%~dp0"
>>"%INSTALLDIR%\run-dxca.cmd" echo dxca.exe ^> run.log 2^>^&1

rem Registered via PowerShell, not `schtasks /create`, so that the path and
rem the working directory survive spaces without any quote gymnastics.
powershell -NoProfile -Command ^
  "$a = New-ScheduledTaskAction -Execute '%INSTALLDIR%\run-dxca.cmd' -WorkingDirectory '%INSTALLDIR%';" ^
  "$t = New-ScheduledTaskTrigger -AtStartup;" ^
  "$p = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest;" ^
  "$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit 0;" ^
  "Register-ScheduledTask -TaskName '%TASKNAME%' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null" >nul
if errorlevel 1 (
  echo [X] Could not register the scheduled task.
  pause
  exit /b 1
)
echo [ok] Scheduled task "%TASKNAME%" registered ^(LOCAL SYSTEM, starts at boot^).

schtasks /run /tn "%TASKNAME%" >nul
set "UP2="
for /l %%i in (1,1,20) do (
  if not defined UP2 (
    powershell -NoProfile -Command "Start-Sleep -Milliseconds 700" >nul
    curl.exe -s -m 3 -o nul http://127.0.0.1:%WEBPORT%/ && set "UP2=1"
  )
)
if not defined UP2 (
  echo [X] The task was registered but the server is not answering.
  echo     Check %INSTALLDIR%\run.log
  echo.
  pause
  exit /b 1
)

echo.
echo ==========================================================
if defined UPGRADE (echo  Updated and running.) else (echo  Installed and running.)
echo ==========================================================
echo   This PC   : http://127.0.0.1:%WEBPORT%/
if defined UPGRADE (
  echo   Config    : unchanged — bind address is in config\dxca.toml
  echo   Loggers   : telnet DX cluster on port %TELNETPORT% as before
)
rem Only advertise a LAN address when one will actually answer. Printing it
rem whenever --expose was chosen would promise reachability that a Public
rem network profile silently withholds.
if defined LANOK (
  for /f "tokens=2 delims=:" %%a in ('ipconfig ^| findstr /C:"IPv4 Address"') do (
    for /f "tokens=* delims= " %%b in ("%%a") do echo   On the LAN: http://%%b:%WEBPORT%/
  )
  echo   Loggers   : point telnet DX cluster at this PC, port %TELNETPORT%
) else (
  if "%EXPOSE%"=="1" (
    echo   On the LAN: NOT reachable — network profile is not Private.
    echo               Fix that above, then re-run this installer.
  )
)
echo.
echo   Folder: %INSTALLDIR%
echo   Start : schtasks /run /tn %TASKNAME%
echo   Stop  : schtasks /end /tn %TASKNAME%
echo   Log   : %INSTALLDIR%\run.log
echo   Remove: uninstall-dxca.cmd
echo.
echo   To upgrade later: unzip the new release anywhere and run
echo   install-dxca.cmd. It installs into the folder above, so your
echo   settings, account and log matrix carry over by themselves.
echo   The folder you unzipped is not needed afterwards.
echo.
echo   Reminder: ClubLog and Telegram credentials are stored in
echo   data\dxca.db in PLAIN TEXT. See README-WINDOWS.txt.
echo.
pause
endlocal
