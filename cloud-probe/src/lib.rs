//! cloud-probe 库（感知 → 七维翻译；逻辑与平台采集分离）。

use meta_kernel_core::l5_senses::FieldReading;

/// 采集后按需回传网关（--report http://host:port → POST /v1/probe）。
/// 平台无关（零依赖 HTTP POST；超时保护）。
pub fn post_probe(endpoint: &str, json: &str) -> Result<String, String> {
    let base = endpoint.trim_end_matches('/');
    let rest = base
        .strip_prefix("http://")
        .ok_or_else(|| "report 需为 http://host:port[/path]".to_string())?;
    // 兼容：端点含路径（如 /v1/probe）则 POST 到该路径；否则默认 /v1/probe
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h.to_string(), format!("/{p}")),
        None => (rest.to_string(), "/v1/probe".to_string()),
    };
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let mut s = TcpStream::connect(&host).map_err(|e| format!("连接 {host}: {e}"))?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(4))).ok();
    s.set_write_timeout(Some(std::time::Duration::from_secs(4))).ok();
    let req = format!(
        "POST {path} HTTP/1.1
Host: {host}
Content-Type: application/json
Content-Length: {}
Connection: close

{}",
        json.len(),
        json
    );
    s.write_all(req.as_bytes()).map_err(|e| format!("发送: {e}"))?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| format!("读取: {e}"))?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    if text.starts_with("HTTP/1.1 200") {
        Ok("probe posted".to_string())
    } else {
        Err(format!("网关非 200: {}", text.lines().next().unwrap_or("")))
    }
}

/// 采集并翻译当前场域 → FieldReading（运行即退：单一采样，~0.5s 完成）。
pub fn collect() -> Result<FieldReading, String> {
    #[cfg(windows)]
    {
        win::sample()
    }
    #[cfg(not(windows))]
    {
        Err("cloud-probe: 非 Windows 平台暂无采集器（fallback）".to_string())
    }
}

#[cfg(windows)]
mod win {
    use super::FieldReading;

    #[repr(C)]
    #[derive(Default)]
    struct MemStatusEx {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page: u64,
        ull_avail_page: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_ext: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct FileTime {
        dw_low: u32,
        dw_high: u32,
    }
    impl FileTime {
        fn to_u64(self) -> u64 {
            ((self.dw_high as u64) << 32) | self.dw_low as u64
        }
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(lp: *mut MemStatusEx) -> i32;
        fn GetSystemTimes(lp_idle: *mut FileTime, lp_kernel: *mut FileTime, lp_user: *mut FileTime) -> i32;
        fn GetDiskFreeSpaceExW(dir: *const u16, free: *mut u64, total: *mut u64, avail: *mut u64) -> i32;
    }

    pub fn sample() -> Result<FieldReading, String> {
        let mut ms = MemStatusEx::default();
        ms.dw_length = std::mem::size_of::<MemStatusEx>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut ms) } == 0 {
            return Err("GlobalMemoryStatusEx 失败".into());
        }
        let mem_pct = ms.dw_memory_load as f64;

        let cpu1 = cpu_pct()?;
        std::thread::sleep(std::time::Duration::from_millis(180));
        let cpu2 = cpu_pct()?;

        let free_pct = disk_free_pct()?;
        let (procs, threads) = count_processes_threads()?;

        let t = cpu2.max(cpu1) / 40.0;
        let f = (cpu2 + 15.0) / (cpu1 + 15.0);
        let a = mem_pct / 40.0;
        let jitter = (cpu2 - cpu1).abs() / cpu1.max(1.0);
        let phi = 1.0 + jitter * 8.0;
        let x = (free_pct / 100.0) / 0.5;
        let h = procs as f64 / 100.0;
        let tau = threads as f64 / 300.0;

        Ok(FieldReading {
            s: [t, f, a, phi, x, h, tau],
            at: None,
        })
    }

    fn cpu_pct() -> Result<f64, String> {
        let mut idle = FileTime::default();
        let mut ker = FileTime::default();
        let mut user = FileTime::default();
        if unsafe { GetSystemTimes(&mut idle, &mut ker, &mut user) } == 0 {
            return Err("GetSystemTimes 失败".into());
        }
        let total = ker.to_u64().saturating_add(user.to_u64());
        let busy = total.saturating_sub(idle.to_u64());
        Ok(busy as f64 / total.max(1) as f64 * 100.0)
    }

    fn disk_free_pct() -> Result<f64, String> {
        let cur = std::env::current_dir().map_err(|e| e.to_string())?;
        let drive = cur.to_string_lossy().chars().next().unwrap_or('C');
        let root = format!("{drive}:\\");
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let mut free: u64 = 0;
        let mut total: u64 = 0;
        if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) } == 0 || total == 0 {
            return Err("GetDiskFreeSpaceExW 失败".into());
        }
        Ok(free as f64 / total as f64 * 100.0)
    }

    fn count_processes_threads() -> Result<(u64, u64), String> {
        // Windows 自带管理 shell 计数进程与线程总数
        let script = "$p=Get-Process -ErrorAction SilentlyContinue; Write-Output $p.Count; Write-Output (($p | ForEach-Object { $_.Threads.Count } | Measure-Object -Sum).Sum)";
        let sh = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output()
            .map_err(|e| format!("ps: {e}"))?;
        let text = String::from_utf8_lossy(&sh.stdout);
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.len() < 2 {
            return Err("ps 输出异常".into());
        }
        Ok((lines[0].trim().parse().unwrap_or(80), lines[1].trim().parse().unwrap_or(400)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_probe_mock_roundtrip() {
        // std mock 网关：接受 POST /v1/probe → 200 + body 存储校验
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let got = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let g2 = std::sync::Arc::clone(&got);
        let srv = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = std::io::Read::read(&mut s, &mut buf);
                *g2.lock().unwrap() = String::from_utf8_lossy(&buf).into_owned();
                let resp = "HTTP/1.1 200 OK
Content-Length: 2
Connection: close

ok";
                let _ = std::io::Write::write_all(&mut s, resp.as_bytes());
            }
        });
        let ep = format!("http://{addr}");
        let r = post_probe(&ep, "{\"s\":[]}");
        srv.join().unwrap();
        assert_eq!(r.as_deref(), Ok("probe posted"));
        let req = got.lock().unwrap().clone();
        assert!(req.contains("POST /v1/probe"), "{req}");
    }

    #[test]
    fn post_probe_rejects_bad_endpoint() {
        assert!(post_probe("ftp://x", "{}").is_err());
        assert!(post_probe("http://127.0.0.1:1", "{}").is_err(), "端口 1 拒绝应报错");
    }
}

/// 诊断回放：给定七维场域读数 → L4 戒律判定 + L5 号脉诊断 → schema2 多语言 JSON。
/// （真实采集数据进入诊断链的统一入口；首次以健康启发基线 1.0 判定，
/// 本底场学习机制后续可精化。）
pub fn diagnose_s(s: [f64; 7]) -> Result<String, String> {
    use meta_kernel_core::l4::dimension::FieldState;
    use meta_kernel_core::l4::l4_router::route_state;
    use meta_kernel_core::l5_baseline::BaselineField;
    use meta_kernel_core::l5_diagnosis::Diagnosis;
    use meta_kernel_core::l5_compare::Band;
    let fs = FieldState::from_vec(&s).ok_or("场域向量需 7 维")?;
    let l4 = route_state(&fs, &FieldState::baseline());
    let fields = meta_kernel_core::l5_senses::decompose(&s);
    let bl = BaselineField { earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0, object: "probe-host", established: "health-heuristic" };
    let pattern = meta_kernel_core::l5_compare::compare(&fields, &bl);
    let conclusion = meta_kernel_core::l5_diagnosis::synthesize(&fields, &bl, &pattern);
    let d = Diagnosis {
        schema: 2,
        fields,
        pattern,
        conclusion,
        trace: meta_kernel_core::l5_diagnosis::Traceability {
            baseline_id: "b-health-heuristic".into(),
            object: "probe-host".into(),
            at: "real-field".into(),
            reproducible: true,
        },
    };
    let l4_report = match &l4 {
        Ok(_) => "Pass".to_string(),
        Err(e) => format!("Reject({:?})", e),
    };
    let json = meta_kernel_core::l5_router::to_json(&d);
    Ok(format!("{{\"l4\":\"{l4_report}\",\"diagnosis\":{json}}}"))
}

/// 解析 FieldReading JSON（schema 1：{\"schema\":1,\"s\":[...] }）。
pub fn parse_reading(json: &str) -> Option<[f64; 7]> {
    let key = "\"s\":[";
    let i = json.find(key)?;
    let rest = &json[i + key.len()..];
    let end = rest.find(']')?;
    let mut out = [0.0f64; 7];
    let mut n = 0;
    for part in rest[..end].split(',') {
        let v: f64 = part.trim().parse().ok()?;
        if n < 7 {
            out[n] = v;
            n += 1;
        }
    }
    if n == 7 { Some(out) } else { None }
}
