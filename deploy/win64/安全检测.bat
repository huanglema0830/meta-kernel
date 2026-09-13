@echo off
title ZhengYuan OS - Security Check
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0cloud-security-check.ps1"
echo.
echo Press any key to close...
pause >nul
