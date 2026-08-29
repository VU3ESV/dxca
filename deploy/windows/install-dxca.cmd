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
set "INSTALLDIR=%~dp0"
if "%INSTALLDIR:~-1%"=="\" set "INSTALLDIR=%INSTALLDIR:~0,-1%"

echo.
echo ==========================================================
echo  DXCA for Windows — installer
echo ==========================================================
echo  Folder : %INSTALLDIR%
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
if not exist "%INSTALLDIR%\dxca.exe" (
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

rem --- fresh install, or an update over an existing one? -----------------
rem An existing config + database means this folder is a live install being
rem updated. That distinction decides everything below: an update MUST NOT
rem rewrite config\dxca.toml, because by then it holds the operator's UDP
rem sources, cluster nodes and broadcast destinations as edited in the web
rem GUI's System tab. Overwriting it would silently discard the whole
rem station setup and look like a successful upgrade.
set "UPGRADE="
if exist "%INSTALLDIR%\config\dxca.toml" (
  if exist "%INSTALLDIR%\data\dxca.db" set "UPGRADE=1"
)

if defined UPGRADE (
  echo [ok] Existing install detected — updating in place.
  echo      config\dxca.toml and data\ will NOT be touched.
) else (
  echo [ok] No config or database in this folder.
)

rem --- carry a previous install's settings across -------------------------
rem Every release unzips into its OWN version-named folder, so an operator
rem installing 2.8.0 runs this from a folder that has never held a config.
rem Without this block that reads as a fresh install: a new empty database
rem is created and the account, ClubLog credentials, log matrix and alert
rem history are all left orphaned in the previous version's folder. That is
rem the "every install needs reconfiguring" complaint, and it is data loss
rem rather than mere inconvenience.
rem
rem The reliable mechanism is the operator naming the old folder; detecting
rem it from the existing scheduled task is only a convenience, because the
rem task listing is English-only and its encoding varies by Windows build.
rem Detection failing must never block the import.
if not defined UPGRADE (
  set "OLDDIR="
  for /f "tokens=2*" %%a in (
    'schtasks /query /tn "%TASKNAME%" /v /fo list 2^>nul ^| findstr /i /c:"Task To Run:"'
  ) do set "OLDEXE=%%b"
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

  rem Second look, and in practice the better one: a SIBLING folder. Each
  rem release unzips as dxca-<version>-windows-x64, so successive versions
  rem normally end up side by side under one parent (C:\dxca\dxca-2.8.0-...
  rem next to C:\dxca\dxca-2.9.0-...). Unlike reading the scheduled task,
  rem this works on any language of Windows and needs nothing to be
  rem running. Newest first, and the first one holding a real install wins.
  if not defined SUGGEST (
    for /f "delims=" %%d in ('dir /b /ad /o-d "%INSTALLDIR%\..\dxca-*" 2^>nul') do (
      if not defined SUGGEST (
        set "CAND=%INSTALLDIR%\..\%%d"
        for %%f in ("!CAND!") do set "CAND=%%~ff"
        if /i not "!CAND!"=="%INSTALLDIR%" (
          if exist "!CAND!\config\dxca.toml" if exist "!CAND!\data\dxca.db" set "SUGGEST=!CAND!"
        )
      )
    )
  )

  echo.
  echo  Settings and accounts live in a DXCA folder's config\ and data\.
  echo  A new version unzips to a NEW folder, so they must be carried over
  echo  or this install starts empty and everything needs setting up again.
  if defined SUGGEST echo  Found a previous install: !SUGGEST!
  echo.
  if defined SUGGEST (
    set /p "IMPORT=Import settings from that folder? [Y/n, or type another path] "
    if "!IMPORT!"=="" set "IMPORT=Y"
    if /i "!IMPORT!"=="Y" set "IMPORT=!SUGGEST!"
  ) else (
    set /p "IMPORT=Path of your previous DXCA folder, or blank for a fresh install: "
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
echo   Start : schtasks /run /tn %TASKNAME%
echo   Stop  : schtasks /end /tn %TASKNAME%
echo   Log   : %INSTALLDIR%\run.log
echo   Remove: uninstall-dxca.cmd
echo.
echo   Reminder: ClubLog and Telegram credentials are stored in
echo   data\dxca.db in PLAIN TEXT. See README-WINDOWS.txt.
echo.
pause
endlocal
