# net-repair.ps1 —— 老旧笔记本网络恢复（逐项确认，可回滚）
#
# 用法：在老旧笔记本上双击「老笔记本-网络修复.bat」（会自动请求管理员权限）。
# 原则：
#   - 动手前**先备份**（net-backup-<时间戳>\ 目录）；
#   - 每项**先问再做**；轻量项默认 Y，重项（防火墙/网络栈重置）默认 N；
#   - 所有动作打印出来，便于复盘。
#
# 恢复项：① 重置为 DHCP  ② 清除代理  ③ 恢复 hosts  ④ 清除异常路由（可选）  ⑤ 防火墙（可选）  ⑥ 网络栈重置（可选，需重启）
# 完成后自动复测连通性。

$ErrorActionPreference = 'Continue'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

function Say([string]$s) { Write-Host $s }
function Hr() { Say ('-' * 64) }
function Ask([string]$q, [bool]$defaultYes = $true) {
    $d = if ($defaultYes) { '[Y/n]' } else { '[y/N]' }
    $a = Read-Host ("$q  $d")
    if ([string]::IsNullOrWhiteSpace($a)) { return $defaultYes }
    return ($a.Trim().ToLower() -in @('y','yes','是','1'))
}
function HasAdmin() {
    try {
        $id = [Security.Principal.WindowsIdentity]::GetCurrent()
        return (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    } catch { return $false }
}

Say "================ 网络恢复（逐项确认） ================"
$admin = HasAdmin()
Say ("管理员权限: " + $admin)
if (-not $admin) {
    Say "  [提示] 多数恢复项需要管理员。请改用「老笔记本-网络修复.bat」（会自动提权）。"
    Say "        仍可继续，但部分项会失败。"
    if (-not (Ask "仍要以当前权限继续？" $false)) { return }
}

# ---------- A. 备份 ----------
Hr; Say "【A】备份当前网络配置"
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$bakDir = Join-Path $PSScriptRoot ("net-backup-" + $stamp)
if (Ask "备份到 $bakDir ？") {
    try {
        New-Item -ItemType Directory -Path $bakDir -Force | Out-Null
        (ipconfig /all)  | Out-File (Join-Path $bakDir 'ipconfig-all.txt') -Encoding UTF8
        (route print -4) | Out-File (Join-Path $bakDir 'route-print.txt') -Encoding UTF8
        (netsh interface ipv4 show config) | Out-File (Join-Path $bakDir 'netsh-ipv4-config.txt') -Encoding UTF8
        $hostsPath = "$env:SystemRoot\System32\drivers\etc\hosts"
        if (Test-Path $hostsPath) { Copy-Item $hostsPath (Join-Path $bakDir 'hosts.bak') -Force }
        (reg query 'HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings') | Out-File (Join-Path $bakDir 'proxy-reg.txt') -Encoding UTF8
        Say "  备份完成：$bakDir"
    } catch { Say ("  [警告] 备份出错: " + $_.Exception.Message) }
} else { Say "  已跳过备份（不推荐）" }

# ---------- ① 重置为 DHCP ----------
Hr; Say "【1】重置为自动获取（DHCP）"
if (Ask "将已联网网卡重置为 DHCP（IP 与 DNS 自动获取）？") {
    $names = @()
    try { $names = @(Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | Select-Object -ExpandProperty Name) } catch {}
    if ($names.Count -eq 0) { Say "  (未找到 Up 状态网卡，改用全部适配器名)" }
    foreach ($n in $names) {
        Say ("  重置: " + $n)
        (netsh interface ipv4 set address     name="$n" source=dhcp) 2>&1 | ForEach-Object { Say ("    " + $_) }
        (netsh interface ipv4 set dnsservers  name="$n" source=dhcp) 2>&1 | ForEach-Object { Say ("    " + $_) }
    }
    Say "  ipconfig /release + /renew ..."
    (ipconfig /release) 2>&1 | Out-Null
    (ipconfig /renew)   2>&1 | ForEach-Object { Say ("    " + $_) }
    Say "  DNS 缓存已清（flushdns）"
    (ipconfig /flushdns) 2>&1 | Out-Null
} else { Say "  已跳过" }

# ---------- ② 清除代理 ----------
Hr; Say "【2】清除系统代理 / WinHTTP 代理"
if (Ask "关闭 IE/系统代理 并重置 WinHTTP 代理？") {
    try {
        $p = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
        Set-ItemProperty -Path $p -Name ProxyEnable -Value 0 -Type DWord -Force
        Set-ItemProperty -Path $p -Name ProxyServer -Value '' -Force -ErrorAction SilentlyContinue
        Say "  已关闭系统代理（ProxyEnable=0）"
    } catch { Say ("  [警告] 注册表写入失败: " + $_.Exception.Message) }
    (netsh winhttp reset proxy) 2>&1 | ForEach-Object { Say ("    " + $_) }
} else { Say "  已跳过" }

# ---------- ③ 恢复 hosts ----------
Hr; Say "【3】恢复 hosts 文件"
$hostsPath = "$env:SystemRoot\System32\drivers\etc\hosts"
if (Ask "把 hosts 中的非标准重定向条目注释掉（保留备份）？") {
    try {
        if (Test-Path $hostsPath) {
            Copy-Item $hostsPath (Join-Path $bakDir 'hosts.before-repair.bak') -Force -ErrorAction SilentlyContinue
            $lines = Get-Content $hostsPath
            $outLines = New-Object System.Collections.Generic.List[string]
            $outLines.Add('# 由 net-repair.ps1 于 ' + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss') + ' 重置')
            $outLines.Add('127.0.0.1  localhost')
            $outLines.Add('::1        localhost')
            $commented = 0
            foreach ($l in $lines) {
                $t = $l.Trim()
                if ($t -eq '' -or $t.StartsWith('#') -or $t -match '^\s*(127\.0\.0\.1|::1)\s+localhost\s*$') { continue }
                $outLines.Add('# [已注释] ' + $l)
                $commented++
            }
            $outLines | Set-Content -Path $hostsPath -Encoding ASCII
            Say ("  hosts 已重置；注释掉 {0} 条非标准条目" -f $commented)
        } else { Say "  hosts 不存在，已跳过" }
    } catch { Say ("  [警告] hosts 处理失败（可能需管理员）: " + $_.Exception.Message) }
} else { Say "  已跳过" }

# ---------- ④ 清除异常默认路由 ----------
Hr; Say "【4】默认路由检查"
$defs = @()
try { $defs = @(Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | Sort-Object RouteMetric) } catch {}
if ($defs.Count -eq 0) {
    Say "  当前无默认路由——请确认网卡已获取到网关（可能需重新 DHCP；见【1】）"
} else {
    foreach ($r in $defs) { Say ("  经 {0}  接口 {1}  度量 {2}" -f $r.NextHop, $r.ifIndex, $r.RouteMetric) }
    if ($defs.Count -gt 1) {
        if (Ask "存在多条默认路由，删除除首条外的其它默认路由？" $false) {
            for ($i = 1; $i -lt $defs.Count; $i++) {
                $r = $defs[$i]
                (route delete 0.0.0.0 mask 0.0.0.0 $r.NextHop) 2>&1 | ForEach-Object { Say ("    " + $_) }
            }
            Say "  已尝试删除多余默认路由"
        } else { Say "  已跳过" }
    } else { Say "  默认路由唯一，正常" }
}

# ---------- ⑤ 防火墙（重项） ----------
Hr; Say "【5】防火墙（重项）"
Say "  说明：netsh advfirewall reset 会把防火墙规则**恢复出厂默认**（自定义规则将丢失）。"
if (HasAdmin() -and (Ask "要把防火墙恢复为默认策略吗？（默认否，慎用）" $false)) {
    (netsh advfirewall reset) 2>&1 | ForEach-Object { Say ("    " + $_) }
    Say "  防火墙已重置为默认"
} else { Say "  已跳过（推荐先只关闭/加白名单，而不是整体重置）" }

# ---------- ⑥ 网络栈重置（重项，需重启） ----------
Hr; Say "【6】网络栈重置（重项，需重启）"
Say "  说明：winsock reset + int ip reset 会重建 TCP/IP 与 Winsock 目录，**需重启生效**。"
if (HasAdmin() -and (Ask "要重置网络栈吗？（默认否）" $false)) {
    (netsh winsock reset) 2>&1 | ForEach-Object { Say ("    " + $_) }
    (netsh int ip reset)   2>&1 | ForEach-Object { Say ("    " + $_) }
    Say "  [!] 请**重启电脑**后再生效。"
} else { Say "  已跳过" }

# ---------- 复测 ----------
Hr; Say "【复测】连通性"
function TestTcp([string]$h, [int]$port, [int]$ms = 2500) {
    try {
        $c = New-Object System.Net.Sockets.TcpClient
        $iar = $c.BeginConnect($h, $port, $null, $null)
        if ($iar.AsyncWaitHandle.WaitOne($ms, $false) -and $c.Connected) { $c.Close(); return $true }
        $c.Close(); return $false
    } catch { return $false }
}
$gw = @()
try { $gw = @(Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty NextHop -Unique) } catch {}
foreach ($g in $gw) { if ($g) { Say ("  网关 {0} : {1}" -f $g, $(if (TestTcp $g 80 1500) {'可达'} else {'不可达'})) } }
Say ("  公网 223.5.5.5:443 : " + $(if (TestTcp '223.5.5.5' 443) { '可达' } else { '不可达' }))
try {
    $r = Resolve-DnsName 'www.baidu.com' -ErrorAction Stop
    $a = ($r | Where-Object { $_.IPAddress } | Select-Object -First 2 -ExpandProperty IPAddress) -join ', '
    Say ("  DNS 解析 baidu : 成功 -> {0}" -f $a)
} catch { Say "  DNS 解析 baidu : 失败" }

Hr
$out = Join-Path $PSScriptRoot ('net-repair-report-' + $stamp + '.txt')
Say ("备份目录: " + $bakDir)
Say "完成。若仍不通，请把 net-backup-*/ 与 net-report.txt 带回开发机。"
Write-Host ""
Write-Host "按回车键关闭..." -ForegroundColor Yellow
try { [void][System.Console]::ReadLine() } catch {}
