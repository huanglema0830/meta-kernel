@echo off
title Cloud Kernel - Enable Auto Report
cd /d "%~dp0"
cscript //nologo "%~dp0enable-autoreport.vbs"
echo.
echo Press any key to close...
pause >nul
