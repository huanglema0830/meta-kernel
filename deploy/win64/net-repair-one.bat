@echo off
chcp 65001 >nul
rem ============================================================
rem  net-repair-one.bat —— 单文件入口（自身下载修复脚本并提权运行）
rem  用法：双击运行；或带参数指定开发机地址：
rem        net-repair-one.bat http://192.168.1.4:3000
rem  说明：先备份、再逐项确认；轻项默认 Y，重项默认 N。
rem ============================================================
setlocal enabledelayedexpansion
net session >nul 2>&1
if errorlevel 1 (
  echo 正在请求管理员权限 ...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs" 2>nul
  if errorlevel 1 (
    echo [提示] 提权被取消——可右键“以管理员身份运行”。
    pause
  )
  exit /b
)

set "ORIGIN=%~1"
if "%ORIGIN%"=="" set "ORIGIN=http://192.168.1.4:3000"
set "D=%TEMP%\ck-net"
if not exist "%D%" mkdir "%D%" >nul 2>&1

echo [1/2] 从 %ORIGIN% 获取修复脚本 ...
curl.exe -s -f -o "%D%\net-repair.ps1" "%ORIGIN%/net-repair.ps1" 2>nul
if not exist "%D%\net-repair.ps1" (
  powershell -NoProfile -Command "(New-Object Net.WebClient).DownloadFile('%ORIGIN%/net-repair.ps1','%D%\net-repair.ps1')" 2>nul
)
if not exist "%D%\net-repair.ps1" (
  echo [错误] 下载失败——请确认开发机网关在运行、IP 正确（192.168.1.4）。
  pause
  exit /b 1
)

echo [2/2] 运行网络修复（逐项确认） ...
powershell -NoProfile -ExecutionPolicy Bypass -File "%D%\net-repair.ps1"
