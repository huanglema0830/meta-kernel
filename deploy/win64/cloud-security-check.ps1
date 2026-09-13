# ============================================================
#  正源操作系统 · 安全软件检测
#  Cloud Security Check
#
#  检测本机安全软件状态，评估「可能拦截网关」的风险等级，
#  并给出白名单建议（只建议，不自动执行）。
#
#  用法：
#    安全检测.bat                                   （双击，显示完整报告）
#    powershell -File cloud-security-check.ps1 -Quiet （只写报告文件）
# ============================================================
param(
    [switch]$Quiet
)

$ErrorActionPreference = 'SilentlyContinue'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

$Base = $PSScriptRoot
$ReportPath = Join-Path $Base 'security-report.txt'
$lines = New-Object System.Collections.ArrayList

function Emit($s) {
    [void]$lines.Add([string]$s)
    if (-not $Quiet) { Write-Host $s }
}
function Hr { Emit ('-' * 62) }

$risks = New-Object System.Collections.ArrayList
function Add-Risk($name, $level, $blocks, $note) {
    [void]$risks.Add([pscustomobject]@{
        Name = $name; Level = $level; Blocks = $blocks; Note = $note
    })
}

Emit ""
Emit "  =============================================================="
Emit "   正源操作系统  ·  安全软件检测 (Cloud Security Check)"
Emit "  =============================================================="
Emit ""
Emit ("  时间: " + (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'))
Emit ("  主机: " + $env:COMPUTERNAME + "    部署目录: " + $Base)
Emit ""

# ---------------------------------------------------------------
# 1) Windows Defender
# ---------------------------------------------------------------
Hr
Emit " [1] Windows Defender"
Hr
try {
    $mp = Get-MpComputerStatus -ErrorAction Stop
    Emit ("  反病毒已启用       : " + $mp.AntivirusEnabled)
    Emit ("  实时保护           : " + $mp.RealTimeProtectionEnabled)
    Emit ("  篡改保护           : " + $mp.IsTamperProtected)
    Emit ("  引擎版本           : " + $mp.AMEngineVersion)
    if ($mp.RealTimeProtectionEnabled) {
        Add-Risk 'Windows Defender' 'LOW' '否' '实时保护开启；首次运行本地 exe/bat 可能被扫描（一般不影响）'
    } else {
        Add-Risk 'Windows Defender' 'NONE' '否' '实时保护未开启'
    }
} catch {
    Emit "  未检测到（可能被第三方杀软接管或系统精简）"
    Add-Risk 'Windows Defender' 'NONE' '否' '未安装 / 已让位第三方'
}

# ---------------------------------------------------------------
# 2) 防火墙
# ---------------------------------------------------------------
Hr
Emit " [2] 防火墙 (Firewall)"
Hr
$fwPrivate = $null
try {
    foreach ($p in (Get-NetFirewallProfile -ErrorAction Stop)) {
        $st = if ($p.Enabled) { 'ON' } else { 'off' }
        Emit ("  {0,-8} : {1}" -f $p.Name, $st)
        if ($p.Name -eq 'Private') { $fwPrivate = [bool]$p.Enabled }
    }
} catch {
    # fallback A: CIM
    try {
        foreach ($p in (Get-CimInstance -Namespace 'root\StandardCimv2' -ClassName MSFT_NetFirewallProfile -ErrorAction Stop)) {
            $st = if ($p.Enabled) { 'ON' } else { 'off' }
            $nm = switch ($p.Name) { 0 { 'Domain' } 1 { 'Private' } 2 { 'Public' } default { "$($p.Name)" } }
            Emit ("  {0,-8} : {1}" -f $nm, $st)
            if ($nm -eq 'Private') { $fwPrivate = [bool]$p.Enabled }
        }
    } catch {
        # fallback B: registry (no admin needed)
        $map = @{
            Domain  = 'DomainProfile'
            Private = 'StandardProfile'
            Public  = 'PublicProfile'
        }
        foreach ($k in $map.Keys) {
            $path = "HKLM:\SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\$($map[$k])"
            $v = (Get-ItemProperty $path -Name EnableFirewall -ErrorAction SilentlyContinue).EnableFirewall
            if ($null -ne $v) {
                $st = if ($v -eq 1) { 'ON' } else { 'off' }
                Emit ("  {0,-8} : {1}   (registry)" -f $k, $st)
                if ($k -eq 'Private') { $fwPrivate = ($v -eq 1) }
            }
        }
        if ($null -eq $fwPrivate) {
            Emit "  无法读取防火墙配置（已尝试 WMI / CIM / 注册表）"
        }
    }
}
if ($null -eq $fwPrivate) {
    Add-Risk 'Firewall(Private)' 'UNKNOWN' '未知' '未能读取，无法判断局域网是否被拦'
} elseif ($fwPrivate) {
    Add-Risk 'Firewall(Private)' 'MEDIUM' '是' '专用网络防火墙开启 —— 其他设备访问 3000 可能被拦（建议放行规则）'
} else {
    Add-Risk 'Firewall(Private)' 'NONE' '否' '专用网络防火墙关闭 —— 局域网可直连'
}

# ---------------------------------------------------------------
# 3) 第三方杀软（SecurityCenter2）
# ---------------------------------------------------------------
Hr
Emit " [3] 第三方杀毒软件 (SecurityCenter2)"
Hr
$thirdParty = @()
try {
    $avs = Get-CimInstance -Namespace 'root\SecurityCenter2' -ClassName AntiVirusProduct -ErrorAction Stop
    if (-not $avs) { $avs = Get-WmiObject -Namespace 'root\SecurityCenter2' -Class AntiVirusProduct }
    $seen = @{}
    foreach ($a in $avs) {
        $name = [string]$a.displayName
        if ($name -like '*Windows Defender*') { continue }
        if ($seen.ContainsKey($name)) { continue }   # 去重（多版本同装）
        $seen[$name] = $true
        $thirdParty += $name
        Emit ("  名称 : " + $name)
        if ($a.pathToSignedProductExe) { Emit ("  路径 : " + $a.pathToSignedProductExe) }
        Emit ("  状态 : 0x" + ('{0:X}' -f $a.productState))
        Emit ""
    }
} catch {
    Emit "  无法读取（非管理员或被系统限制）"
}
if ($thirdParty.Count -eq 0) {
    Emit "  未检测到第三方杀软"
    Add-Risk 'Third-party AV' 'NONE' '否' '无第三方杀软'
} else {
    $names = ($thirdParty -join ', ')
    Add-Risk 'Third-party AV' 'HIGH' '是' ("检测到: $names —— 常拦截本地 HTTP 监听 / 未知 exe / 脚本自启")
    Emit ("  >> 这些软件往往会拦截：新程序的网络监听、vbs/ps1 脚本、注册表自启写入。")
    Emit ("  >> 若网关反复被关闭或脚本无效，请优先检查它的「信任区 / 白名单」。")
    Emit ""
}

# ---------------------------------------------------------------
# 4) SmartScreen
# ---------------------------------------------------------------
Hr
Emit " [4] SmartScreen"
Hr
$ss = $null
try { $ss = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer' -Name SmartScreenEnabled -ErrorAction Stop).SmartScreenEnabled } catch { }
if ($null -eq $ss) { try { $ss = (Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' -Name EnableSmartScreen -ErrorAction Stop).EnableSmartScreen } catch { } }
if ($ss) {
    Emit ("  SmartScreenEnabled : " + $ss)
    if ("$ss" -ne 'Off') {
        Add-Risk 'SmartScreen' 'LOW' '否' '开启 —— 下载的 exe/bat 首次运行会弹「已阻止」（右键→属性→解除锁定）'
    } else {
        Add-Risk 'SmartScreen' 'NONE' '否' '已关闭'
    }
} else {
    Emit "  未配置（默认策略）"
    Add-Risk 'SmartScreen' 'LOW' '否' '未显式配置，通常仍会对下载文件提示'
}

# ---------------------------------------------------------------
# 5) UAC
# ---------------------------------------------------------------
Hr
Emit " [5] UAC (用户账户控制)"
Hr
try {
    $lua = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' -Name EnableLUA -ErrorAction Stop).EnableLUA
    Emit ("  EnableLUA : " + $lua)
    Add-Risk 'UAC' 'NONE' '否' '本方案全部走 HKCU，无需提权，不受影响'
} catch {
    Emit "  读取失败"
    Add-Risk 'UAC' 'UNKNOWN' '否' '读取失败'
}

# ---------------------------------------------------------------
# 6) 网关可达性自检
# ---------------------------------------------------------------
Hr
Emit " [6] 网关可达性自检"
Hr
$gwUp = $false
try {
    $c = New-Object System.Net.Sockets.TcpClient
    $c.Connect('127.0.0.1', 3000)
    $gwUp = $c.Connected
    $c.Close()
} catch { }
if ($gwUp) {
    Emit "  127.0.0.1:3000  : 在线 (gateway running)"
    try {
        $wc = New-Object System.Net.WebClient
        $wc.Proxy = $null
        $h = $wc.DownloadString('http://127.0.0.1:3000/v1/health')
        if ($h) { Emit ("  /v1/health      : " + $h) }
    } catch { }
} else {
    Emit "  127.0.0.1:3000  : 未运行（双击 一键启动.bat 启动）"
}
$lanIp = (Get-NetIPAddress -AddressFamily IPv4 |
          Where-Object { $_.IPAddress -match '^(192\.168\.|10\.|172\.(1[6-9]|2[0-9]|3[01])\.)' } |
          Select-Object -First 1).IPAddress
if ($lanIp) { Emit ("  本机局域网地址   : " + $lanIp + ":3000  （其他设备用此地址）") }

# ---------------------------------------------------------------
# 风险汇总
# ---------------------------------------------------------------
Emit ""
Hr
Emit " 风险汇总"
Hr
Emit ("  {0,-20} {1,-8} {2,-14} {3}" -f '检测项', '风险', '可能拦截网关', '说明')
$rank = @{ 'NONE' = 0; 'LOW' = 1; 'UNKNOWN' = 1; 'MEDIUM' = 2; 'HIGH' = 3 }
$worst = 'NONE'
foreach ($r in $risks) {
    Emit ("  {0,-20} {1,-8} {2,-14} {3}" -f $r.Name, $r.Level, $r.Blocks, $r.Note)
    if ($rank[$r.Level] -gt $rank[$worst]) { $worst = $r.Level }
}
Emit ""
Emit ("  >>> 综合拦截风险等级: " + $worst)
switch ($worst) {
    'HIGH'   { Emit "      网关有较高概率被安全软件干扰 —— 请按下方建议添加信任。" }
    'MEDIUM' { Emit "      局域网访问可能受阻 —— 建议放行 3000 端口。" }
    'LOW'    { Emit "      影响很小 —— 首次运行可能弹提示，选择允许即可。" }
    default  { Emit "      未发现明显拦截风险。" }
}

# ---------------------------------------------------------------
# 白名单建议（不自动执行）
# ---------------------------------------------------------------
Emit ""
Hr
Emit " 白名单建议（仅供参考，未自动执行）"
Hr
Emit "  1) 把整个部署目录加入安全软件「信任区 / 排除项」："
Emit ("       " + $Base)
Emit "     （火绒：主界面 → 安全工具 → 信任区 → 添加目录）"
Emit "  2) 放行入站 TCP 3000（专用网络）——需管理员身份运行一次："
Emit '       netsh advfirewall firewall add rule name="ZhengYuan Gateway" dir=in action=allow protocol=TCP localport=3000 profile=private'
Emit "  3) 若 SmartScreen 拦截下载的 bat/exe：右键 → 属性 → 勾选「解除锁定」→ 确定。"
Emit "  4) 安全软件弹窗询问时，选择「允许」并勾选「记住本次操作」。"
Emit ""

try {
    $utf8Bom = New-Object System.Text.UTF8Encoding($true)
    [System.IO.File]::WriteAllText($ReportPath, ($lines -join "`r`n"), $utf8Bom)
    if (-not $Quiet) { Emit ("  报告已保存: " + $ReportPath) }
} catch {
    if (-not $Quiet) { Emit ("  报告写入失败: " + $_) }
}

if ($Quiet) { Write-Output ("SECURITY_RISK=" + $worst) }
