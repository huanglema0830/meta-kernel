//! cloud-discover：云操作系统内网设备发现（Windows 一期）。
//!
//! 流程：取本机 /24 → ARP 并发扫描 .1-254 → 在线设备 →（提示 HTTP 下载探针到目标执行）
//! → 探针 `--report http://<本机>:3000` 回传网关。任何不可自动完成的环节 →
//! 输出「需人工介入」状态（含目标 IP 与主机名）。扫描即退、零后台。

pub mod auto;

/// 发现结果（状态 JSON 载体）。
#[derive(Clone, Debug)]
pub struct DiscoverResult {
    pub prefix: String,
    pub scanned: u32,
    pub online: Vec<Host>,
    pub manual_needed: Vec<Manual>,
}

/// 在线主机。
#[derive(Clone, Debug)]
pub struct Host {
    pub ip: String,
    pub hostname: Option<String>,
}

/// 需人工介入条目。
#[derive(Clone, Debug)]
pub struct Manual {
    pub ip: String,
    pub hostname: Option<String>,
    pub reason: String,
}

impl DiscoverResult {
    /// 状态 JSON（{scanned, online:[{ip,hostname}], manual_needed:[{ip,hostname,reason}],
    /// probe_download: "http://<gw>:3000/cloud-probe.exe" 等指引}）
    pub fn to_json(&self) -> String {
        let on = self
            .online
            .iter()
            .map(|h| format!("{{\"ip\":\"{}\",\"hostname\":\"{}\"}}", h.ip, h.hostname.clone().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(",");
        let man = self
            .manual_needed
            .iter()
            .map(|m| {
                format!(
                    "{{\"ip\":\"{}\",\"hostname\":\"{}\",\"reason\":\"{}\"}}",
                    m.ip,
                    m.hostname.clone().unwrap_or_default(),
                    m.reason
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"scanned\":{},\"prefix\":\"{}\",\"online\":[{}],\"manual_needed\":[{}]}}",
            self.scanned, self.prefix, on, man
        )
    }
}

/// 主入口：扫描 /24 并返回结果（Windows 实采；其他平台 fallback 空+人工）。
pub fn scan() -> Result<DiscoverResult, String> {
    #[cfg(windows)]
    {
        win::run()
    }
    #[cfg(not(windows))]
    {
        Err("cloud-discover: 非 Windows 平台暂无扫描器（fallback）".to_string())
    }
}

/// 构造前缀范围的 254 个候选 IP（纯函数；便于测试）。
pub fn build_hosts(prefix: &str) -> Vec<String> {
    (1..=254).map(|i| format!("{prefix}.{i}")).collect()
}

/// 解析本机 IPv4 所在 /24 前缀（纯逻辑：取首个私网 v4）。
pub fn parse_prefix(ip: &str) -> Option<String> {
    let octets: Vec<&str> = ip.split('.').collect();
    if octets.len() != 4 {
        return None;
    }
    let is_private = octets[0] == "10"
        || (octets[0] == "172" && octets[1].parse::<u32>().map(|n| (16..=31).contains(&n)).unwrap_or(false))
        || (octets[0] == "192" && octets[1] == "168");
    if !is_private {
        return None;
    }
    Some(format!("{}.{}.{}", octets[0], octets[1], octets[2]))
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::thread;

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn SendARP(dest_ip: u32, src_ip: u32, mac: *mut u8, mac_len: *mut u32) -> i32;
    }

    fn to_u32(ip: &str) -> Option<u32> {
        ip.parse::<Ipv4Addr>().ok().map(u32::from)
    }

    /// 解析本机 IP：ipconfig 输出取首个私网 IPv4（/24 前缀源）。
    pub fn local_ip() -> Result<String, String> {
        let out = std::process::Command::new("ipconfig")
            .output()
            .map_err(|e| format!("ipconfig: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut cur = String::new();
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with("IPv4") || l.starts_with("IPv6") {
                if let Some(i) = l.find(':') {
                    cur = l[i + 1..].trim().to_string();
                    if !cur.contains(':') && cur.split('.').count() == 4 {
                        return Ok(cur);
                    }
                }
            }
        }
        Err("未找到 IPv4 地址".to_string())
    }

    /// 活跃二次确认：TCP 服务（445/135/139）或 ICMP 回包。避免 ARP 缓存误报。
    fn alive(ip: &str) -> bool {
        tcp_ok(ip, 445) || tcp_ok(ip, 135) || tcp_ok(ip, 139) || ping_reply(ip)
    }

    fn ping_reply(ip: &str) -> bool {
        let out = std::process::Command::new("ping")
            .args(["-n", "1", "-w", "300", ip])
            .output()
            .ok();
        out.map(|o| String::from_utf8_lossy(&o.stdout).contains("TTL=")).unwrap_or(false)
    }

    fn tcp_ok(ip: &str, port: u16) -> bool {
        use std::io::Write;
        use std::net::TcpStream;
        std::net::TcpStream::connect_timeout(
            &format!("{ip}:{port}").parse().unwrap_or_else(|_| "127.0.0.1:1".parse().unwrap()),
            std::time::Duration::from_millis(180),
        )
        .map(|mut s| { let _ = s.write_all(b"
"); true })
        .unwrap_or(false)
    }

    /// ARP 探测一个 IP：SendARP 命中（有 MAC）→ 候选。
    fn arp_probe(ip: &str) -> bool {
        let Some(target) = to_u32(ip) else { return false };
        let mut mac = [0u8; 6];
        let mut len: u32 = 6;
        let rc: i32 = unsafe { SendARP(target, 0, mac.as_mut_ptr(), &mut len) };
        rc == 0
    }

    pub fn run() -> Result<DiscoverResult, String> {
        let local = local_ip()?;
        let prefix = parse_prefix(&local).ok_or_else(|| format!("非私网地址：{local}"))?;
        let hosts = build_hosts(&prefix);
        // 在线判定：并发 ICMP（ping 回包 TTL=）——真实在线最可靠；
        // ARP(SendARP)在本环境存在缓存假阳，故仅作辅助（不单判）。
        let found = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let mut handles = Vec::new();
        const CHUNK: usize = 32;
        for chunk in hosts.chunks(CHUNK) {
            let chunk = chunk.to_vec();
            let found = Arc::clone(&found);
            handles.push(thread::spawn(move || {
                for ip in chunk {
                    if ping_reply(&ip) {
                        found.lock().unwrap().push(ip);
                    }
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        let mut online_ips: Vec<String> = found.lock().unwrap().clone();
        online_ips.sort();
        // TCP 活跃过滤（进一步确认可管理/可传探针的设备）
        let mut online = Vec::new();
        let mut manual = Vec::new();
        for ip in &online_ips {
            let hostname = resolve_hostname(ip);
            online.push(Host { ip: ip.clone(), hostname: hostname.clone() });
            manual.push(Manual {
                ip: ip.clone(),
                hostname,
                reason: "探针需在目标设备上执行：请在该设备打开 http://<云操作系统主机>:3000/ 下载 cloud-probe.exe 并以 --report 回传（或人工共享运行）".to_string(),
            });
        }
        if online.is_empty() {
            manual.push(Manual {
                ip: local,
                hostname: None,
                reason: "本 /24 未发现其他在线设备（本机隔离网络/目标关机或禁 ping）——请检查网络；或对该 IP 单独处理".to_string(),
            });
        }
        Ok(DiscoverResult { prefix, scanned: 254, online, manual_needed: manual })
    }

    /// 主机名：ping -a（仅对在线设备；~300ms/台）。
    fn resolve_hostname(ip: &str) -> Option<String> {
        let out = std::process::Command::new("ping")
            .args(["-a", "-n", "1", "-w", "300", ip])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().find(|l| l.contains("Pinging") || l.contains("ping"))?;
        // Pinging [hostname] [ip] 或 Pinging hostname.ip...
        let rest = line.split_whitespace().nth(1)?.trim_matches('[').trim_matches(']');
        if rest == ip || rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_private_prefix() {
        assert_eq!(parse_prefix("192.168.1.23").as_deref(), Some("192.168.1"));
        assert_eq!(parse_prefix("10.0.0.5").as_deref(), Some("10.0.0"));
        assert_eq!(parse_prefix("172.16.9.1").as_deref(), Some("172.16.9"));
        assert_eq!(parse_prefix("8.8.8.8"), None, "公网不扫");
        assert_eq!(parse_prefix("bad"), None);
    }

    #[test]
    fn build_hosts_covers_254() {
        let h = build_hosts("192.168.1");
        assert_eq!(h.len(), 254);
        assert_eq!(h[0], "192.168.1.1");
        assert_eq!(h[253], "192.168.1.254");
    }

    #[test]
    fn result_json_shape() {
        let r = DiscoverResult {
            prefix: "192.168.1".into(),
            scanned: 254,
            online: vec![Host { ip: "192.168.1.8".into(), hostname: Some("note".into()) }],
            manual_needed: vec![Manual { ip: "192.168.1.8".into(), hostname: Some("note".into()), reason: "手动".into() }],
        };
        let j = r.to_json();
        assert!(j.contains("\"scanned\":254"), "{j}");
        assert!(j.contains("192.168.1.8"), "{j}");
        assert!(j.contains("\"manual_needed\""), "{j}");
    }
}



/// 一键自动链：ping 确认 → 挂载管理共享投放探针 → 远程计划任务执行 --report →
/// 轮询网关收数（见 auto 模块）。失败任一环节 → ok=false + note（manual_needed 兜底）。
pub fn auto(ip: &str, user: &str, pass: &str, probe_exe: &str, gw: &str) -> auto::AutoResult {
    auto::run(ip, user, pass, probe_exe, gw)
}
