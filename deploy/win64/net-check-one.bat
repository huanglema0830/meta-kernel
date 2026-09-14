@echo off
chcp 65001 >nul
rem ============================================================
rem  net-check-one.bat —— 单文件入口（自身下载诊断脚本并运行）
rem  用法：双击运行；或带参数指定开发机地址：
rem        net-check-one.bat http://192.168.1.4:3000
rem  说明：只读排查，不修改任何网络配置。
rem ============================================================
setlocal enabledelayedexpansion
set "ORIGIN=%~1"
if "%ORIGIN%"=="" set "ORIGIN=http://192.168.1.4:3000"
set "D=%TEMP%\ck-net"
if not exist "%D%" mkdir "%D%" >nul 2>&1

echo [1/2] 从 %ORIGIN% 获取诊断脚本 ...
curl.exe -s -f -o "%D%\net-diagnose.ps1" "%ORIGIN%/net-diagnose.ps1" 2>nul
if not exist "%D%\net-diagnose.ps1" (
  powershell -NoProfile -Command "(New-Object Net.WebClient).DownloadFile('%ORIGIN%/net-diagnose.ps1','%D%\net-diagnose.ps1')" 2>nul
)
if not exist "%D%\net-diagnose.ps1" (
  echo [错误] 下载失败——请确认：
  echo        1^) 开发机网关正在运行（开发机上双击 一键启动.bat）
  echo        2^) 开发机 IP 正确（当前为 192.168.1.4）
  echo        3^) 老笔记本与开发机在同一局域网
  echo   也可用参数指定：net-check-one.bat http://^<开发机IP^>:3000
  pause
  exit /b 1
)

echo [2/2] 运行网络排查（只读） ...
powershell -NoProfile -ExecutionPolicy Bypass -File "%D%\net-diagnose.ps1"
