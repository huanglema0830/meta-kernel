@echo off
rem ASCII 别名（供网关/局域网直接下载：http://<开发机IP>:3000/net-check.bat）
chcp 65001 >nul
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0net-diagnose.ps1"
