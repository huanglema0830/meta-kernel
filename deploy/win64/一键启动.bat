@echo off
title Cloud Kernel - One Click Launch
cd /d "%~dp0"

echo.
echo   ==================================================
echo    Cloud Kernel  /  Konghai Browser
echo   ==================================================
echo.

rem --- 1/3 start watchdog (hidden, idempotent) ---
start "" wscript.exe "%~dp0watchdog.vbs"
echo   [1/3] Watchdog started  (keeps gateway alive / auto-restart)

rem --- 2/3 wait until port 3000 is listening ---
echo   [2/3] Waiting for gateway on port 3000 ...
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
start "CloudKernelGateway" /min "%~dp0npb-gateway.exe" 0.0.0.0:3000 --ui "%~dp0ui"
ping -n 4 127.0.0.1 >nul

:ready
echo   [3/3] Opening browser  http://127.0.0.1:3000/
start "" "http://127.0.0.1:3000/"
echo.
echo   Ready. You may close this window (gateway runs in background).
timeout /t 3 >nul
exit
