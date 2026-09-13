@echo off
title Cloud Kernel Gateway (close this window to stop)
cd /d %~dp0
echo.
echo   Starting gateway on ALL network interfaces, port 3000 ...
echo.
echo   This machine (local):  http://127.0.0.1:3000/
echo   Other devices (LAN):   http://THIS-PC-IP:3000/
echo.
ipconfig | findstr /c:"IPv4"
echo.
start "" http://127.0.0.1:3000/
npb-gateway.exe 0.0.0.0:3000 --ui ui
pause
