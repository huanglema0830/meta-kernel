@echo off
title ZhengYuan OS - One Click Launch
cd /d "%~dp0"

echo.
echo   ==================================================
echo    ZhengYuan OS   /   ZhengYuan Browser
echo   ==================================================
echo.

rem --- 1/4 security software check (quiet; writes security-report.txt) ---
echo   [1/4] Checking security software ...
set "SECRISK="
for /f "tokens=2 delims==" %%r in ('powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0cloud-security-check.ps1" -Quiet 2^>nul') do set "SECRISK=%%r"
if defined SECRISK (
  echo         interception risk = %SECRISK%   ^(details: security-report.txt^)
) else (
  echo         risk = unknown  ^(run 安全检测.bat for details^)
)

rem --- 2/4 start watchdog (hidden, idempotent) ---
start "" wscript.exe "%~dp0watchdog.vbs"
echo   [2/4] Watchdog started  ^(keeps gateway alive / auto-restart^)

rem --- 3/4 wait until port 3000 is listening ---
echo   [3/4] Waiting for gateway on port 3000 ...
set /a n=0
:wait
netstat -ano | find ":3000" | find "LISTENING" >nul 2>&1
if not errorlevel 1 goto ready
set /a n+=1
if %n% geq 20 goto timeout
ping -n 2 127.0.0.1 >nul
goto wait

:timeout
echo   [warn] Not up yet - starting directly ...
start "ZhengYuanGateway" /min "%~dp0npb-gateway.exe" 0.0.0.0:3000 --ui "%~dp0ui"
ping -n 4 127.0.0.1 >nul

:ready
echo   [4/4] Opening browser  http://127.0.0.1:3000/
start "" "http://127.0.0.1:3000/"
echo.
echo   Ready. You may close this window (gateway runs in background).
timeout /t 3 >nul
exit
