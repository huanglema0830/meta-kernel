@echo off
title ZhengYuan OS - Auto Collect (old notebook)
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0autoprobe.ps1"
echo.
echo Press any key to close...
pause >nul
