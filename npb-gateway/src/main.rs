//! npb-gateway 二进制入口：本地启动 HTTP/SSE 网关（默认 127.0.0.1:8080）。
//!
//! 用法：`cargo run -p npb-gateway [PORT]`

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let mut server = match npb_gateway::http::spawn(port) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("gateway bind error: {e}");
            std::process::exit(1);
        }
    };
    println!("meta-kernel gateway listening on http://{}", server.addr);
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
