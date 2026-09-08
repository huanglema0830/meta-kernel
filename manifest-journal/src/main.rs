//! Manifest Journal CLI（最小版本）：连接常驻网关，把命令行文本跑一轮显化并打印日志流。
//!
//! 用法：
//!   cargo run -p manifest-journal -- <gateway_addr> "<一条念头文本>"
//!   例：cargo run -p manifest-journal -- 127.0.0.1:3000 "把今天的灵感归档"
//!   （gateway 可本地：cargo run -p npb-gateway）

use manifest_journal::{JournalSession, band_name};
use npb_appkit::EventPipe;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: manifest-journal <gateway_addr> \"念头文本\"");
        std::process::exit(2);
    }
    let addr = args[1].clone();
    let text = args[2..].join(" ");

    // 订阅网关 SSE
    let pipe = match EventPipe::subscribe(&addr) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("网关不可达 {addr}: {e}");
            std::process::exit(1);
        }
    };
    let mut session = JournalSession::new(&text);
    println!("条目   : {} seed={:.4}", session.entry.id, session.entry.seed);
    println!("原文   : {}", session.entry.raw);
    println!("--- 显化轮 ---");
    session.run_pushes(&addr, &pipe.rx, 40);
    println!("显化态 : {} (轮次 {})", band_name(session.engine.state), session.engine.rounds);
    println!("--- 显化日志流（最近 8 条，均可溯源） ---");
    for l in session.log.iter().rev().take(8).rev() {
        println!("  [{:>3}] {}", l.lifecycle, l.statement);
    }
    if let Some(i) = &session.last_intent {
        println!("意图建议: {i}");
    }
    println!("完成。可 seed 重放：{}", session.entry.seed);
}
