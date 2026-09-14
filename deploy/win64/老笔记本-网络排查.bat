@echo off
chcp 65001 >nul
cd /d "%~dp0"
echo ============================================
echo   老旧笔记本 · 网络排查（只读，不改配置）
echo ============================================
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0net-diagnose.ps1"
