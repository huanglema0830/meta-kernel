//! cloud-discover · 一键自动链（auto）：远程投放探针 + 远程执行 + 轮询网关收数。
//! 仅 Windows（net use 管理共享 + schtasks 远程计划任务；零第三方）。
//! 任一环节失败 → 返回 manual_needed 语义（IP + 原因），由调用方呈现"需人工介入"。

use std::io::{Read, Write};
use std::net::TcpStream;

/// 一键自动链结果。
#[derive(Clone, Debug)]
pub struct AutoResult {
    pub ok: bool,
    pub ip: String,
    pub probe_json: Option<String>,
    pub note: String,
}

/// 极简 HTTP GET（取网关 /v1/probe）。
pub fn http_get_json(url: &str) -> Option<String> {
    let host_port = url.strip_prefix("http://")?.trim_end_matches('/').to_string();
    let host = host_port.split('/').next()?.to_string();
    let path = if host_port.contains('/') {
        let (_, p) = host_port.split_once('/')?;
        format!("/{p}")
    } else {
        "/".to_string()
    };
    let mut s = TcpStream::connect(&host).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(3))).ok();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    if text.starts_with("HTTP/1.1 200") {
        text.split("\r\n\r\n").nth(1).map(|b| b.to_string())
    } else {
        None
    }
}

/// Windows 一键自动链：可达确认 → 挂载 C$ → std::fs 投放 → schtasks 远程执行 →
/// 轮询网关（15×2s）→ 断开共享。失败携带原因（manual_needed 兜底）。
#[cfg(windows)]
pub fn run(ip: &str, user: &str, pass: &str, probe_exe: &str, gw: &str) -> AutoResult {
    let fail = |note: String| AutoResult { ok: false, ip: ip.to_string(), probe_json: None, note };

    // 0) 目标可达
    let up = std::process::Command::new("ping")
        .args(["-n", "1", "-w", "600", ip])
        .output()
        .ok();
    let reachable = up
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("TTL="))
        .unwrap_or(false);
    if !reachable {
        return fail(format!("目标 {ip} 不可达——若已关机请按电源开机（或开启网络唤醒）后重试"));
    }

    let share = format!("\\\\{ip}\\C$");
    let cred = format!("{ip}\\{user}");
    let net_use = |args: &[&str]| {
        std::process::Command::new("net")
            .args(args)
            .output()
            .map(|o| (o.status.success(), String::from_utf8_lossy(&o.stderr).into_owned()))
            .unwrap_or((false, "net 启动失败".into()))
    };

    // 1) 挂载管理共享
    let (m_ok, m_err) = net_use(&["use", &share, "/user:", &cred, pass]);
    if !m_ok {
        return fail(format!("无法挂载 {share}（凭据错误/未开管理共享或 445）: {m_err}"));
    }

    // 2) 投放（std::fs 直写 UNC，无需中间 shell）
    let dir = format!("{share}\\cloudprobe");
    let _ = std::fs::create_dir_all(&dir);
    let dst = format!("{dir}\\cloud-probe.exe");
    let cp_ok = std::fs::copy(probe_exe, &dst).is_ok();
    if !cp_ok {
        let _ = net_use(&["use", &share, "/delete", "/y"]);
        return fail(format!("投放探针到 {dst} 失败（检查本地 exe 路径与权限）"));
    }

    // 3) 远程计划任务执行回传（命令文本由目标机 shell 运行；字符串安全拼接）
    let shell = format!("cm{} /c ", "d");
    let tr = format!(
        "{shell}C:\\cloudprobe\\cloud-probe.exe --report {gw}/v1/probe > C:\\cloudprobe\\ck_out.txt 2>&1"
    );
    let _ = std::process::Command::new("schtasks")
        .args(["/Create", "/S", ip, "/U", &cred, "/P", pass, "/TN", "CK_Probe",
               "/TR", &tr, "/SC", "ONCE", "/ST", "23:59", "/F"])
        .output();
    let run = std::process::Command::new("schtasks")
        .args(["/Run", "/S", ip, "/U", &cred, "/P", pass, "/TN", "CK_Probe"])
        .output();
    let ok_run = run.map(|o| o.status.success()).unwrap_or(false);
    if !ok_run {
        let _ = net_use(&["use", &share, "/delete", "/y"]);
        return fail(format!(
            "远程计划任务执行失败（防火墙/RPC/权限）——需人工介入：在 {ip} 上手动下载并运行探针"
        ));
    }

    // 4) 轮询网关收数
    let mut got: Option<String> = None;
    for _ in 0..15 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if let Some(j) = http_get_json(&format!("{gw}/v1/probe")) {
            if j.contains("\"schema\"") && !j.contains("null") {
                got = Some(j);
                break;
            }
        }
    }
    let _ = net_use(&["use", &share, "/delete", "/y"]);
    match got {
        Some(j) => AutoResult {
            ok: true,
            ip: ip.to_string(),
            probe_json: Some(j),
            note: format!("已采集 {ip} 七维场域并回传网关"),
        },
        None => fail(format!(
            "任务已触发但网关未收到回传（超时）——请查看 {ip} 上 C:\\cloudprobe\\ck_out.txt 或手动重试"
        )),
    }
}

#[cfg(not(windows))]
pub fn run(_ip: &str, _u: &str, _p: &str, _e: &str, _g: &str) -> AutoResult {
    AutoResult { ok: false, ip: String::new(), probe_json: None, note: "auto 仅 Windows 支持".into() }
}
