@echo off
title ZhengYuan OS - Disable Auto-Start / Stop All
cd /d "%~dp0"
cscript //nologo "%~dp0disable-autostart.vbs"
echo.
echo Press any key to close...
pause >nul
