//! cloud-discover 入口：扫描本机 /24 → 状态 JSON（含在线设备与需人工介入项），运行即退。
//! 用法：`cloud-discover [--report <gateway-http>]`（可选注入网关地址到指引文案）。

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "--auto" {
        let mut ip = String::new();
        let mut user = String::new();
        let mut pw = String::new();
        let mut probe = "cloud-probe.exe".to_string();
        let mut gw = "http://127.0.0.1:3000".to_string();
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--user" => { i += 1; if i < args.len() { user = args[i].clone(); } }
                "--pass" => { i += 1; if i < args.len() { pw = args[i].clone(); } }
                "--probe" => { i += 1; if i < args.len() { probe = args[i].clone(); } }
                "--gw" => { i += 1; if i < args.len() { gw = args[i].clone(); } }
                other => { if ip.is_empty() { ip = other.to_string(); } }
            }
            i += 1;
        }
        if ip.is_empty() || user.is_empty() || pw.is_empty() {
            eprintln!("用法: cloud-discover --auto <目标IP> --user <用户名> --pass <密码> [--probe <本地探针exe路径>] [--gw http://<开发机IP>:3000]");
            std::process::exit(2);
        }
        let r = cloud_discover::auto(&ip, &user, &pw, &probe, &gw);
        match r.probe_json {
            Some(j) => {
                println!("{{\"ok\":true,\"note\":\"{}\",\"probe\":{j}}}", r.note);
                // 自动诊断：真实场域 → L4/L5 → schema2 多语言结论
                if let Some(s7) = cloud_probe::parse_reading(&j) {
                    if let Ok(diag) = cloud_probe::diagnose_s(s7) {
                        println!("{diag}");
                    }
                }
            }
            None => println!("{{\"ok\":false,\"manual_needed\":{{\"ip\":\"{}\",\"reason\":\"{}\"}}}}", r.ip, r.note),
        }
        return;
    }
    let report_gw = args.get(2).cloned().unwrap_or_default();
    match cloud_discover::scan() {
        Ok(r) => {
            let mut json = r.to_json();
            if !report_gw.is_empty() {
                json = json.replace("\"manual_needed\"", &format!("\"gateway\":\"{report_gw}\",\"manual_needed\""));
            }
            println!("{json}");
            // 控制台可读摘要
            for h in &r.online {
                println!("[在线] {} ({})", h.ip, h.hostname.clone().unwrap_or_else(|| "名称未知".into()));
            }
            if r.online.is_empty() {
                println!("[提示] 未发现其他在线设备（仅本机网段自身）");
            }
            println!("[探针指引] 在目标设备运行：cloud-probe.exe --report http://<云操作系统主机>:3000");
        }
        Err(e) => {
            eprintln!("cloud-discover error: {e}");
            std::process::exit(1);
        }
    }
}
