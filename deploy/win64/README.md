# Windows 一键部署与自动化（deploy/win64）

目标：**用户只管双击，不接触任何 IP / 端口 / 命令行概念。**

## 用户可见的入口（只有 5 个）

| 入口 | 在哪台机器 | 作用 |
|---|---|---|
| `一键启动.bat` | 开发机 | 启动守护 → 确保网关 → **自动打开浏览器** |
| `设置-开机自启.bat` | 开发机 | 写入 `HKCU\...\Run`（**免管理员**）→ 开机自动起网关 |
| `取消-开机自启并停止.bat` | 开发机 | 撤销自启 + 停掉守护与网关 |
| `老笔记本-一键采集.bat` | 老笔记本 | **自动扫描局域网找到网关** → 下载探针 → 采集回传 |
| `老笔记本-安装开机自动上报.bat` | 老笔记本 | 安装一次，之后**每次开机自动上报** |

## 内部机制

```
watchdog.vbs           隐藏窗口守护循环：每 10s 检查 npb-gateway.exe，
                       不在就拉起（0.0.0.0:3000 --ui ui）。单实例自保护。
enable-autostart.vbs   写 HKCU Run → "watchdog.vbs"，并立即启动它。
disable-autostart.vbs  删 Run 项 + 终止 watchdog/gateway（WMI 精确匹配）。
autoprobe.ps1          ①取本机私网前缀 ②并发扫描 x.1–x.254:3000
                       ③用 /v1/health 确认网关 ④下载探针 ⑤--report 回传
autoprobe-hidden.vbs   隐藏运行 autoprobe.ps1（登录后延迟 25s 等网络就绪）。
enable/disable-autoreport.vbs  老笔记本侧的自启开关。
```

## 为什么这样设计

- **不用 IP**：开发机 IP 会随 DHCP 漂移（已实测 `.3 → .4`）。笔记本侧改为**扫描网段 + /v1/health 验证**自动定位，网关侧 `run-probe.bat` 也改为**按请求 Host 动态生成**——两侧都不依赖固定地址。
- **不用管理员**：全部走 `HKCU`（当前用户），无需提权、可一键撤销。
- **崩溃自愈**：watchdog 每 10s 巡检，实测**网关被强杀后 8s 内自动恢复**。
- **零窗口干扰**：守护与自动上报都以隐藏窗口（`Run(..., 0, ...)`）运行。
- **下载兼容性**：`autoprobe.ps1` 与网关生成的 bat **不用 Invoke-WebRequest**——PS 5.1 的该 cmdlet 会拒绝极简 HTTP 服务器（报 `协议冲突 Section=ResponseStatusLine`）；改用 `curl.exe`（Win10 1803+ 自带）优先、`.NET WebClient` 兜底。

## 实测验收（2026-09-14）

| 项 | 结果 |
|---|---|
| 守护启动网关 | ✅ 4 秒内 |
| 崩溃自动重启 | ✅ 强杀后 8 秒内恢复 |
| 开机自启注册 | ✅ `HKCU\...\Run\CloudKernelGateway = "...\watchdog.vbs"` |
| 笔记本自动发现 | ✅ 扫描 `<内网网段>/24` → 命中网关 → 下载 1382KB → `probe posted` 回传 |
| 动态 bat | ✅ 经不同 Host 访问分别生成对应地址 |

## 注意

- 自启注册的是**当前部署包路径**，若移动/删除 `CloudKernel-Run` 文件夹，需重新运行一次 `设置-开机自启.bat`。
- 老笔记本需与开发机连同一路由器；自动扫描耗时约 2 秒。
