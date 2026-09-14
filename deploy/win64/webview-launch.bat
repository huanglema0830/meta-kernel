@echo off
chcp 65001 >nul
rem ============================================================
rem  空天浏览器 · WebView2 宿主启动器（v0.112）
rem  策略（发起人 Q1/Q3 裁决）：
rem    ① 检测 WebView2 Runtime；② 有 → 启动宿主（只读+叠层）
rem    ③ 无 → 若有离线 Runtime 安装包则安装；④ 仍不行 → **降级 B**（默认浏览器打开，不见白屏）
rem  安全：不修改任何网络配置；不静默安装；安装需用户确认（可能弹 UAC）。
rem ============================================================
setlocal enabledelayedexpansion
set "DIR=%~dp0"
set "HOST=%DIR%sky-browser.exe"
set "RT_INSTALLER=%DIR%MicrosoftEdgeWebView2RuntimeInstaller.exe"
set "PORT=3000"
set "URL=http://127.0.0.1:%PORT%/?host=webview"

echo ===== [1/4] 检测 WebView2 Runtime =====
set "RT="
for /f "tokens=2,*" %%A in ('reg query "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" /v pv 2^>nul ^| findstr pv') do set "RT=%%B"
if not defined RT (
  for /f "tokens=2,*" %%A in ('reg query "HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" /v pv 2^>nul ^| findstr pv') do set "RT=%%B"
)
if defined RT (echo   已安装 WebView2 Runtime 版本: !RT!) else (echo   未检测到 WebView2 Runtime)

echo ===== [2/4] 有宿主且有 Runtime → 直接启动（只读+叠层）=====
if defined RT if exist "%HOST%" (
  echo   启动空天浏览器宿主: %HOST%
  start "" "%HOST%" --url "http://127.0.0.1:%PORT%/" --readonly
  goto :done
)
if not defined RT echo   （Runtime 缺失，跳过宿主）
if not exist "%HOST%" echo   （尚未提供宿主程序 sky-browser.exe —— 见 docs 待办）

echo ===== [3/4] Runtime 缺失时：尝试离线安装包 =====
if not defined RT (
  if exist "%RT_INSTALLER%" (
    echo   发现离线安装包: %RT_INSTALLER%
    echo   即将运行安装（可能弹出 UAC 授权窗口，请允许）...
    "%RT_INSTALLER%" /silent /install
    echo   安装返回码: !errorlevel!
  ) else (
    echo   未找到离线安装包 MicrosoftEdgeWebView2RuntimeInstaller.exe
  )
)

echo ===== [4/4] 降级 B：用系统默认浏览器打开工作台（不让用户见白屏）=====
echo   打开 %URL%
start "" "http://127.0.0.1:%PORT%/"

:done
echo.
echo 说明：本脚本**未修改任何网络配置**（IP/DNS/代理/hosts/防火墙/路由）。
pause
