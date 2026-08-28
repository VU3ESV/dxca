@echo off
setlocal
cd /d "%~dp0"

rem ---------------------------------------------------------------------
rem DXCA Windows uninstaller — removes the scheduled task and the firewall
rem rules that install-dxca.cmd created.
rem
rem It deliberately does NOT delete config\ or data\. Your accounts,
rem ClubLog matrices and settings live there; deleting them is a separate,
rem conscious act. Remove this whole folder by hand when you mean it.
rem
rem Run this by RIGHT-CLICKING it and choosing "Run as administrator".
rem ---------------------------------------------------------------------

set "TASKNAME=dxca"

echo.
echo ==========================================================
echo  DXCA for Windows — uninstaller
echo ==========================================================
echo.

whoami /groups | findstr /C:"S-1-16-12288" >nul
if errorlevel 1 (
  echo [X] This uninstaller must run elevated.
  echo     Right-click uninstall-dxca.cmd and choose "Run as administrator".
  echo.
  pause
  exit /b 1
)

echo Stopping the service...
schtasks /end /tn "%TASKNAME%" >nul 2>&1
taskkill /im dxca.exe /f >nul 2>&1
echo [ok] Stopped.

echo Removing the scheduled task...
schtasks /delete /tn "%TASKNAME%" /f >nul 2>&1
if errorlevel 1 (
  echo [--] No scheduled task named "%TASKNAME%" was present.
) else (
  echo [ok] Scheduled task removed.
)

echo Removing firewall rules...
netsh advfirewall firewall delete rule name="dxca-webui"  >nul 2>&1
netsh advfirewall firewall delete rule name="dxca-telnet" >nul 2>&1
echo [ok] Firewall rules removed.

echo.
echo Done. Your data was left untouched:
echo   %~dp0config
echo   %~dp0data
echo.
echo data\dxca.db still holds your account password hashes and any ClubLog
echo or Telegram credentials IN PLAIN TEXT. Delete this folder if you are
echo handing the machine on.
echo.
pause
endlocal
