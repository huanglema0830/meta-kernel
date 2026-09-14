@echo off
chcp 65001 >nul
cd /d "%~dp0"
echo ============================================
echo   老旧笔记本 · 网络修复（逐项确认，可回滚）
echo ============================================
echo.
net session >nul 2>&1
if errorlevel 1 (
  echo 正在请求管理员权限...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0net-repair.ps1"
