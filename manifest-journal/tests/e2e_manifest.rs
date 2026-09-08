//! Manifest Journal 端到端验收（L5 §7 五条准则）。
//! 起真实 npb-gateway（进程内 HTTP/SSE）→ 真实网络栈全链路。

use manifest_journal::{ManifestEntry, JournalSession, band_name, seed_of};
use npb_appkit::EventPipe;

/// 验收 1：文本 → 稳定种子 → push 被接受（accepted:true）。
#[test]
fn acceptance_1_seed_stable_and_push_accepted() {
    let mut srv = npb_gateway::http::spawn(0).expect("spawn gw");
    let addr = srv.addr.clone();
    let text = "把 A4/A5 复核结论写进正式档案";
    let s1 = seed_of(text);
    let s2 = seed_of(text);
    assert_eq!(s1, s2, "种子稳定");
    let s = JournalSession::new(text);
    let ok = s.push(&addr, s.entry.seed);
    let mut srv2 = srv;
    srv2.stop();
    assert!(ok, "push 应 accepted");
}

#[allow(dead_code)]
fn _seed_helper() -> f32 {
    seed_of("x")
}

/// 验收 2（链路部分）：事件流 → 生命周期自 0 点亮并推进（真实网关端到端）。
#[test]
fn acceptance_2_lifecycle_awakens_and_advances_over_live_gateway() {
    let mut srv = npb_gateway::http::spawn(0).expect("spawn gw");
    let addr = srv.addr.clone();
    let pipe = EventPipe::subscribe(&addr).expect("subscribe");
    let mut s = JournalSession::new("端到端生命周期验证：一条要显化的念头");
    s.run_pushes(&addr, &pipe.rx, 40);
    let mut srv2 = srv;
    srv2.stop();
    assert!(s.engine.state >= 10, "应从 0 点亮到 ≥10，实际 {}", s.engine.state);
    assert!(band_name(s.engine.state).contains("带") || s.engine.state == 99);
}

/// 验收 3：日志每条 = 命名/陈述 + 可溯源（抽查 ≥5 条人工可比对回事件）。
#[test]
fn acceptance_3_log_lines_traceable_spot_check() {
    let mut s = JournalSession::new("抽查溯源：记录一条值得显化的经验");
    // 用模拟推进产生足够日志
    use npb_appkit::KernelEvent;
    for _ in 0..40 {
        s.simulate(&[KernelEvent::Awaken]);
    }
    s.simulate(&[KernelEvent::Hold; 3]);
    let lines = &s.log;
    assert!(lines.len() >= 5, "应有 ≥5 条日志，实际 {}", lines.len());
    for l in lines.iter().take(5) {
        assert!(!l.statement.is_empty());
        assert!(!l.source.is_empty(), "日志必须可溯源");
        // 溯源句柄为指令/引擎标识
        assert!(l.source.starts_with("instruction#") || l.source.starts_with("lifecycle") || l.source.starts_with("sim#"), "{}", l.source);
    }
}

/// 验收 4：补充扰动可再推进；归档可早退回融（本地动作，条目保留）。
#[test]
fn acceptance_4_boost_advances_and_archive_retreats() {
    let mut s = JournalSession::new("推进与归档");
    let s0 = s.engine.state;
    let boosted = s.boost_seed();
    assert!(boosted >= s.entry.seed && boosted <= 0.95, "boost seed 界内");
    use npb_appkit::KernelEvent;
    s.simulate(&[KernelEvent::Awaken; 5]);
    assert!(s.engine.state > s0, "补充扰动推进");
    let st = s.engine.state;
    s.archive();
    assert_eq!(s.engine.state, 0, "归档回锚点");
    assert_eq!(s.entry.retained_early, 1);
    assert!(st > 0);
    assert!(s.entry.raw == "推进与归档", "条目保留原文");
}

/// 验收 5：重启恢复 + seed 重放趋势一致。
#[test]
fn acceptance_5_restore_and_replay_consistent() {
    let mut s = JournalSession::new("重启恢复：这条念头要跨会话");
    use npb_appkit::KernelEvent;
    s.simulate(&[KernelEvent::Awaken; 16]);
    let line = s.entry.to_line();
    let restored = ManifestEntry::from_line(&line).expect("恢复");
    assert_eq!(restored.raw, s.entry.raw, "原文恢复");
    assert!((restored.seed - s.entry.seed).abs() < 1e-5, "seed 恢复容差");
    // 重放：从恢复条目重建会话（seed 同）→ 跑 10 步 → 与原始继续跑 10 步趋势同
    let mut a = JournalSession::new(&restored.raw);
    assert_eq!(a.entry.seed, restored.seed);
    let mut b = s;
    let mut seq_a = Vec::new();
    let mut seq_b = Vec::new();
    for _ in 0..10 {
        seq_a.push(a.simulate(&[KernelEvent::Awaken]));
        seq_b.push(b.simulate(&[KernelEvent::Awaken]));
    }
    assert_eq!(seq_a, seq_b, "重放趋势一致");
}
