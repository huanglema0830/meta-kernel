//! npb-gateway 二进制入口：本地启动 HTTP/SSE 网关（默认 127.0.0.1:8080）。
//!
//! 用法：`cargo run -p npb-gateway [PORT]`

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // 用法：npb-gateway [PORT|IP:PORT] [--ui <dir>]
    let mut ui_dir: Option<String> = None;
    let mut port: u16 = 3000;
    let mut idx = 1;
    while idx < args.len() {
        let lower = args[idx].to_ascii_lowercase();
        match lower.as_str() {
            "--ui" | "-ui" => {
                idx += 1;
                if idx < args.len() { ui_dir = Some(args[idx].clone()); }
            }
            _ if lower.starts_with("--ui=") => {
                ui_dir = Some(lower.trim_start_matches("--ui=").to_string());
            }
            other => {
                if let Some(p) = other.rsplit(':').next().and_then(|s| s.parse().ok()) {
                    port = p;
                } else if let Ok(p) = other.parse() {
                    port = p;
                }
            }
        }
        idx += 1;
    }
    let mut server = match npb_gateway::http::spawn_custom(port, ui_dir.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("gateway bind error: {e}");
            std::process::exit(1);
        }
    };
    println!("cloud-kernel gateway listening on http://{}", server.addr);
    if ui_dir.is_some() {
        println!("   UI (空海浏览器):  http://{}/  （--ui 同源托管）", server.addr);
    }
    println!("try:  curl -s http://{}/v1/health", server.addr);
    println!("      curl -s http://{}/v1/state", server.addr);
    println!("      curl -s -X POST http://{}/v1/push -d '{{\"seed\":0.5}}'", server.addr);
    println!("      curl -N http://{}/v1/events   (SSE 订阅)", server.addr);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
        // 保持进程存活；stop 由外部信号/退出处理（一期 Ctrl+C 直接结束）
        let _ = &mut server;
    }
}
