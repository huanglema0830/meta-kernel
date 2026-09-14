# net-diagnose.ps1 —— 老旧笔记本网络排查（七项，只读，不改动任何配置）
#
# 用法（在老旧笔记本上）：
#   右键“使用 PowerShell 运行”，或双击同目录的「老笔记本-网络排查.bat」。
# 输出：屏幕报告 + 同目录 net-report.txt（可带回开发机分析）。
#
# 检查项：① 静态IP/DHCP ② DNS ③ 代理 ④ hosts ⑤ 防火墙 ⑥ 路由表 ⑦ 网卡状态
# 另附：网关连通性 + 公网连通性 + DNS 解析实测。
#
# 纯只读：不做任何修改（修改请用 net-repair.ps1）。

$ErrorActionPreference = 'Continue'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

$report = New-Object System.Collections.Generic.List[string]
function Say([string]$line) { Write-Host $line; $report.Add($line) | Out-Null }
function Hr() { Say ('-' * 64) }

$isAdmin = $false
try {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $isAdmin = (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
} catch {}

Say "================ 网络排查报告（只读） ================"
Say ("时间: " + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Say ("主机: " + $env:COMPUTERNAME + "    用户: " + $env:USERNAME + "    管理员: " + $isAdmin)

# ---------- 0) 网卡一览 ----------
Hr; Say "【0】网卡状态"
try {
    Get-NetAdapter -ErrorAction Stop | Sort-Object ifIndex | ForEach-Object {
        Say ("  [{0}] {1} | 状态={2} | 速率={3} | MAC={4}" -f $_.ifIndex, $_.Name, $_.Status, $_.LinkSpeed, $_.MacAddress)
    }
} catch {
    Get-CimInstance Win32_NetworkAdapter -Filter "NetEnabled=True" | ForEach-Object {
        Say ("  {0} | {1}" -f $_.Name, $_.NetConnectionID)
    }
}

# ---------- 1~2) IP / DHCP / 网关 / DNS ----------
Hr; Say "【1+2】IP 获取方式（DHCP/静态）· 默认网关 · DNS"
$configured = @()
try {
    $cfgs = Get-CimInstance Win32_NetworkAdapterConfiguration -Filter "IPEnabled=True" -ErrorAction Stop
    foreach ($c in $cfgs) {
        $name = $c.Description
        $dhcp = $c.DHCPEnabled
        $mode = if ($dhcp) { "DHCP(自动获取)" } else { "静态(手工设置)" }
        $ips = ($c.IPAddress | Where-Object { $_ -notmatch ':' }) -join ', '
        $gw  = ($c.DefaultIPGateway) -join ', '
        $dns = ($c.DNSServerSearchOrder) -join ', '
        Say ("  <{0}>" -f $name)
        Say ("     获取方式 : {0}" -f $mode)
        Say ("     IPv4     : {0}" -f $ips)
        Say ("     默认网关 : {0}" -f $(if ($gw) { $gw } else { "(无)" }))
        Say ("     DNS      : {0}" -f $(if ($dns) { $dns } else { "(无)" }))
        if (-not $dhcp) { Say "     [!] 该项为静态配置——若不确定来源，建议在修复脚本中重置为 DHCP" }
        $configured += [pscustomobject]@{ Name=$name; DHCP=[bool]$dhcp; DNS=$dns; GW=$gw }
    }
    if ($cfgs.Count -eq 0) { Say "  (未找到已启用 IP 的网卡)" }
} catch {
    Say ("  [警告] CIM 查询失败: " + $_.Exception.Message)
    Say "  回退 netsh："
    (netsh interface ipv4 show config) | ForEach-Object { Say ("    " + $_) }
}

# ---------- 3) 代理 ----------
Hr; Say "【3】系统代理 / WinHTTP 代理"
$proxyEnabled = $false; $proxyServer = ''
try {
    $reg = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' -ErrorAction Stop
    $proxyEnabled = [bool]$reg.ProxyEnable
    $proxyServer  = [string]$reg.ProxyServer
    Say ("  IE/系统代理 启用: {0}" -f $proxyEnabled)
    Say ("  代理服务器 : {0}" -f $(if ($proxyServer) { $proxyServer } else { "(空)" }))
    if ($proxyEnabled) { Say "  [!] 系统代理已开启——若代理不可用，浏览器将无法上网（重点嫌疑）" }
} catch { Say "  [警告] 读取代理注册表失败" }
try {
    $w = (netsh winhttp show proxy) 2>&1 | Out-String
    Say ("  WinHTTP 代理: " + ($w -replace '\s+', ' ').Trim())
} catch { Say "  [警告] netsh winhttp 读取失败" }

# ---------- 4) hosts ----------
Hr; Say "【4】hosts 文件"
$hostsPath = "$env:SystemRoot\System32\drivers\etc\hosts"
try {
    if (Test-Path $hostsPath) {
        $raw = Get-Content $hostsPath -ErrorAction Stop
        $active = $raw | Where-Object { $_.Trim() -ne '' -and -not $_.Trim().StartsWith('#') }
        Say ("  路径: {0}" -f $hostsPath)
        Say ("  有效条目数: {0}" -f @($active).Count)
        if (@($active).Count -gt 0) {
            Say "  [!] 存在有效重定向条目（非注释）——可能劫持域名解析："
            $active | ForEach-Object { Say ("      " + $_) }
        } else {
            Say "  仅注释/空行，无异常重定向（正常）"
        }
        $bytes = (Get-Item $hostsPath).Length
        Say ("  文件大小: {0} 字节" -f $bytes)
    } else { Say "  [警告] hosts 文件不存在（通常不该）" }
} catch { Say ("  [警告] 读取 hosts 失败: " + $_.Exception.Message) }

# ---------- 5) 防火墙 ----------
Hr; Say "【5】防火墙状态"
try {
    $profiles = Get-NetFirewallProfile -ErrorAction Stop
    foreach ($p in $profiles) {
        Say ("  {0,-8} : {1}" -f $p.Name, $(if ($p.Enabled) { 'ON' } else { 'off' }))
    }
} catch {
    try {
        foreach ($p in (Get-CimInstance -Namespace 'root\StandardCimv2' -ClassName MSFT_NetFirewallProfile -ErrorAction Stop)) {
            $nm = switch ($p.Name) { 0 {'Domain'} 1 {'Private'} 2 {'Public'} default {$p.Name} }
            Say ("  {0,-8} : {1}" -f $nm, $(if ($p.Enabled) { 'ON' } else { 'off' }))
        }
    } catch {
        Say "  无法读取（需管理员）；可运行 老笔记本-网络修复.bat 以管理员重试"
    }
}
try {
    $blk = @(Get-NetFirewallRule -Direction Outbound -Action Block -Enabled True -ErrorAction Stop)
    Say ("  出站阻止规则（启用）: {0} 条" -f $blk.Count)
    if ($blk.Count -gt 0) {
        $blk | Select-Object -First 6 | ForEach-Object { Say ("      " + $_.DisplayName) }
        if ($blk.Count -gt 6) { Say ("      ... 共 {0} 条" -f $blk.Count) }
    }
} catch { Say "  （无法统计出站规则，通常需管理员）" }

# ---------- 6) 路由表 ----------
Hr; Say "【6】默认路由（0.0.0.0/0）"
try {
    $def = @(Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction Stop | Sort-Object RouteMetric)
    if ($def.Count -eq 0) {
        Say "  [!] 没有默认路由——无法访问本网段以外的地址（外网必不通）"
    } else {
        foreach ($r in $def) {
            Say ("  经 {0}  接口 {1}  度量 {2}  {3}" -f $r.NextHop, $r.ifIndex, $r.RouteMetric, $(if ($r.Publish -eq 'Yes') {'(发布)'} else {''}))
        }
        if ($def.Count -gt 1) { Say ("  [!] 存在 {0} 条默认路由（可能互相冲突，导致流量走错出口）" -f $def.Count) }
    }
} catch {
    Say "  [警告] Get-NetRoute 失败，回退 route print："
    (route print -4) | Select-Object -First 24 | ForEach-Object { Say ("    " + $_) }
}

# ---------- 7) 连通性实测 ----------
Hr; Say "【7】连通性实测"
function TestTcp([string]$host_, [int]$port, [int]$ms = 2500) {
    try {
        $c = New-Object System.Net.Sockets.TcpClient
        $iar = $c.BeginConnect($host_, $port, $null, $null)
        if ($iar.AsyncWaitHandle.WaitOne($ms, $false) -and $c.Connected) { $c.Close(); return $true }
        $c.Close(); return $false
    } catch { return $false }
}
$gwList = @()
try {
    $gwList = @(Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction Stop | Select-Object -ExpandProperty NextHop -Unique)
} catch {}
if ($gwList.Count -eq 0) { $gwList = @('192.168.1.1') }
foreach ($g in $gwList) {
    if ($g) { Say ("  默认网关 {0} : {1}" -f $g, $(if (TestTcp $g 80 1500) { '可达(TCP80)' } else { '不可达/未响应' })) }
}
Say ("  内网 DNS 114.114.114.114:53 : " + $(if (TestTcp '114.114.114.114' 53) { '可达' } else { '不可达' }))
Say ("  公网 223.5.5.5:443         : " + $(if (TestTcp '223.5.5.5' 443) { '可达' } else { '不可达' }))
Say ("  公网 8.8.8.8:53            : " + $(if (TestTcp '8.8.8.8' 53) { '可达' } else { '不可达' }))
try {
    $r = Resolve-DnsName 'www.baidu.com' -ErrorAction Stop
    $a = ($r | Where-Object { $_.IPAddress } | Select-Object -First 2 -ExpandProperty IPAddress) -join ', '
    Say ("  DNS 解析 www.baidu.com     : 成功 -> {0}" -f $a)
} catch {
    Say "  DNS 解析 www.baidu.com     : 失败（DNS 服务器可能不可用）"
}

# ---------- 结论 ----------
Hr; Say "【结论·自动提示】"
$hints = @()
$badDhcp = @($configured | Where-Object { -not $_.DHCP })
if ($badDhcp.Count -gt 0) { $hints += "有网卡为静态配置（见【1】）——若被改成静态则可能无法上网，建议重置为 DHCP" }
if ($proxyEnabled) { $hints += "系统代理已开启——若代理不可用会导致浏览器无法上网，建议清除代理" }
if (Test-Path $hostsPath) {
    $act = @(Get-Content $hostsPath | Where-Object { $_.Trim() -ne '' -and -not $_.Trim().StartsWith('#') })
    if ($act.Count -gt 0) { $hints += "hosts 存在有效重定向条目——可能劫持域名，建议恢复默认" }
}
if ($hints.Count -eq 0) { Say "  未发现明显异常项。若仍不能上网，请把本报告（net-report.txt）带回开发机进一步分析。" }
else { $i = 1; foreach ($h in $hints) { Say ("  {0}. {1}" -f $i, $h); $i++ }; Say ""; Say "  → 可运行「老笔记本-网络修复.bat」按提示逐项恢复。" }

Hr
$out = Join-Path $PSScriptRoot 'net-report.txt'
try {
    $report -join "`r`n" | Out-File -FilePath $out -Encoding UTF8
    Say ("报告已保存: " + $out)
} catch { Say ("[警告] 报告保存失败: " + $_.Exception.Message) }
Say "完成（本次未修改任何配置）。"
Write-Host ""
Write-Host "按回车键关闭..." -ForegroundColor Yellow
try { [void][System.Console]::ReadLine() } catch {}
