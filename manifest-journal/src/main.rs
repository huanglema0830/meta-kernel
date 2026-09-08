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
    if args.len() >= 2 && args[1] == "diagnose" {
        if args.len() < 4 {
            eprintln!("用法: manifest-journal diagnose <gateway_addr> \"故障描述\"");
            std::process::exit(2);
        }
        return run_diagnose(&args[2], &args[3..].join(" "));
    }
    if args.len() < 3 {
        eprintln!("用法: manifest-journal <gateway_addr> \"念头文本\"  或  manifest-journal diagnose <gateway_addr> \"故障描述\"");
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

/// 诊断模式（云操作系统→空海浏览器链路 CLI 版）：输入故障描述 →
/// 元内核显化（真实网关）→ 云操作系统翻译（内置/外部知识库）→ 输出排查步骤。
fn run_diagnose(addr: &str, text: &str) {
    use manifest_journal::knowledge::KbConfig;
    use manifest_journal::run_diagnosis;
    use npb_appkit::EventPipe;
    let pipe = match EventPipe::subscribe(addr) {
        Ok(p) => p,
        Err(e) => { eprintln!("网关不可达 {addr}: {e}"); std::process::exit(1); }
    };
    let mut sess = manifest_journal::JournalSession::new(text);
    println!("诊断输入 : {}", sess.entry.raw);
    println!("种子     : {:.4}", sess.entry.seed);
    println!("--- 元内核显化 → 云操作系统诊断 ---");
    let kb = KbConfig::load();
    let d = run_diagnosis(&mut sess, addr, &pipe.rx, &kb, 12);
    println!("来源     : {}", d.source);
    println!("排查步骤 :");
    for (i, st) in d.steps.iter().enumerate() {
        println!("  {}. {st}", i + 1);
    }
}
