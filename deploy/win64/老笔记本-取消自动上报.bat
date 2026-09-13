@echo off
title Cloud Kernel - Disable Auto Report
cd /d "%~dp0"
cscript //nologo "%~dp0disable-autoreport.vbs"
echo.
echo Press any key to close...
pause >nul
