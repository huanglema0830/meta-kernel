@echo off
rem ASCII 别名（供网关/局域网直接下载：http://<开发机IP>:3000/net-repair.bat）
chcp 65001 >nul
cd /d "%~dp0"
net session >nul 2>&1
if errorlevel 1 (
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0net-repair.ps1"
