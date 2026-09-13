@echo off
title ZhengYuan OS - Enable Auto-Start
cd /d "%~dp0"
cscript //nologo "%~dp0enable-autostart.vbs"
echo.
echo Press any key to close...
pause >nul
