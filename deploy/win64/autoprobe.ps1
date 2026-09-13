# ============================================================
#  Cloud Kernel  -  Auto Probe  (for the old notebook)
#  1) scans the local /24 to find the gateway (port 3000 + /v1/health)
#  2) downloads cloud-probe.exe from it
#  3) samples the field and reports back
#  No IP / port knowledge required. Fully automatic.
#
#  Download uses curl.exe (built into Win10 1803+) with a .NET
#  WebClient fallback -- Invoke-WebRequest is deliberately avoided
#  because the PS 5.1 version rejects some minimal HTTP servers.
# ============================================================
$ErrorActionPreference = 'SilentlyContinue'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

function Step($m) { Write-Host $m }

Write-Host ""
Write-Host "  =================================================="
Write-Host "   Cloud Kernel  -  Auto Probe"
Write-Host "  =================================================="
Write-Host ""

# ---------- 1) local network ----------
$ip = (Get-NetIPAddress -AddressFamily IPv4 |
       Where-Object { $_.IPAddress -match '^(192\.168\.|10\.|172\.(1[6-9]|2[0-9]|3[01])\.)' } |
       Select-Object -First 1).IPAddress

if (-not $ip) {
    Step "  [x] No LAN connection detected. Please connect to the router/WiFi first."
    exit 2
}
$prefix = $ip -replace '\.\d+$', ''
Step "  [1/4] This machine: $ip     scanning: $prefix.0/24"

# ---------- 2) find the gateway ----------
Step "  [2/4] Scanning LAN for the Cloud Kernel gateway ..."
$clients = New-Object System.Collections.ArrayList
foreach ($i in 1..254) {
    $c = New-Object System.Net.Sockets.TcpClient
    $null = $c.BeginConnect("$prefix.$i", 3000, $null, $null)
    [void]$clients.Add([pscustomobject]@{ IP = "$prefix.$i"; C = $c })
}
Start-Sleep -Milliseconds 1500

$gateway = $null
foreach ($x in $clients) {
    if ($x.C.Connected) {
        try {
            $wc = New-Object System.Net.WebClient
            $wc.Proxy = $null
            $probeTxt = $wc.DownloadString("http://$($x.IP):3000/v1/health")
            if ($probeTxt -match '"ok"') { $gateway = $x.IP; break }
        } catch { }
    }
}
foreach ($x in $clients) { try { $x.C.Close() } catch { } }

if (-not $gateway) {
    Step "  [x] Gateway not found."
    Step "      Make sure the other PC is ON and has Cloud Kernel running."
    exit 3
}
Step "  [ok] Gateway found: $gateway"
$base = "http://${gateway}:3000"

# ---------- 3) download probe ----------
$probe = Join-Path $PSScriptRoot 'cloud-probe.exe'
Step "  [3/4] Downloading probe ..."
$ok = $false

# (a) curl.exe - present on Windows 10 1803+ and Windows 11
$curl = Join-Path $env:SystemRoot 'System32\curl.exe'
if (Test-Path $curl) {
    & $curl -s -f -o "$probe" "$base/cloud-probe.exe"
    if ($LASTEXITCODE -eq 0 -and (Test-Path $probe)) { $ok = $true }
}

# (b) fallback: .NET WebClient
if (-not $ok) {
    try {
        $wc = New-Object System.Net.WebClient
        $wc.Proxy = $null
        $wc.DownloadFile("$base/cloud-probe.exe", "$probe")
        if (Test-Path $probe) { $ok = $true }
    } catch { }
}

if (-not $ok -or -not (Test-Path $probe)) {
    Step "  [x] Download failed (tried curl.exe and WebClient)."
    exit 4
}
$kb = [math]::Round((Get-Item $probe).Length / 1KB)
Step "  [ok] Probe ready ($kb KB)"

# ---------- 4) sample and report ----------
Step "  [4/4] Sampling field and reporting ..."
$out = & $probe --report "$base/v1/probe" 2>&1
$out | ForEach-Object { Write-Host "        $_" }

if (($out | Out-String) -match 'probe posted') {
    Write-Host ""
    Step "  [ok] DONE. Data sent. Open the Konghai Browser on the other PC to see it."
    exit 0
} else {
    Step "  [x] Report failed - check the gateway on the other PC."
    exit 5
}
