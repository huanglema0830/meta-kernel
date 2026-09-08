//! # Manifest Journal（L5 参考应用最小版）
//!
//! 依据：docs/L5_REFERENCE_APP_DESIGN v1.0（发起人审核通过）
//! 端到端：文本 → 确定性种子[0.25,0.95] → push 网关 → SSE 订阅 → 生命周期仪表 → 显化日志流。
//! 戒律（审计层 L5）：原文零修改、日志可溯源（source）、seed 可重放、归档=早退回融。
//!
//! 布局：`seed_of`（文本指纹→种子）、`ManifestEntry`（条目）、`JournalSession`（会话：
//! 引擎+日志+意图）、CLI 入口见 `main.rs`，验收端到端见 `tests/e2e_manifest.rs`。

use npb_appkit::event_pipe::RawEvent;
use npb_appkit::httpc;
use npb_appkit::speaker::{Speaker, Statement};
use npb_appkit::{KernelEvent, LifecycleEngine, Namer};

/// 极简 JSON 字段提取（state 快照轮询用；协议自控，无嵌套）。
fn json_val(s: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let idx = s.find(&needle)?;
    let rest = &s[idx + needle.len()..];
    let colon = rest.find(':')? + 1;
    let v: String = rest[colon..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+' || *c == '"' || c.is_alphabetic())
        .collect();
    Some(v.trim_matches('"').to_string())
}

/// 快照探针：GET /v1/state 直读投影（次事件源；守"界面=内核真实投影"）。
#[derive(Clone, Copy, Debug)]
struct SnapshotProbe {
    budget: u32,
    stored: f32,
    low: bool,
}

impl SnapshotProbe {
    fn fetch(addr: &str) -> Option<Self> {
        let body = httpc::get_json(addr, "/v1/state").ok()?;
        let budget = json_val(&body, "budget")?.parse().ok()?;
        let stored: f32 = json_val(&body, "stored")?.parse().ok()?;
        let low = json_val(&body, "low_energy").as_deref() == Some("true");
        Some(Self { budget, stored, low })
    }

    /// 前后快照差 → KernelEvent（预算 code 降=Awaken/升=Settle；持平按储备幅度）。
    fn event(&self, prev: Option<&SnapshotProbe>) -> KernelEvent {
        let Some(p) = prev else { return KernelEvent::Awaken }; // 首探视为点亮探针
        if self.low {
            return KernelEvent::Settle;
        }
        if self.budget != p.budget {
            return if self.budget < p.budget { KernelEvent::Awaken } else { KernelEvent::Settle };
        }
        let d = self.stored - p.stored;
        if d > 0.02 {
            KernelEvent::Awaken
        } else if d < -0.02 {
            KernelEvent::Settle
        } else {
            KernelEvent::Hold
        }
    }
}

/// 日志行（命名 + 陈述 + 来源；审计锚点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalLine {
    pub t: u64,
    pub lifecycle: u16,
    pub statement: String,
    pub source: String,
}

/// 显化条目（应用侧；原文零修改）。
#[derive(Clone, Debug)]
pub struct ManifestEntry {
    pub id: String,
    /// 用户原文（绝不改写）。
    pub raw: String,
    /// 确定性种子。
    pub seed: f32,
    /// 生命周期（0 或 10..=99）。
    pub lifecycle: u16,
    /// 完成轮次（99→0）。
    pub rounds: u32,
    /// 早退回融计数（归档）。
    pub retained_early: u32,
}

/// 文本 → 种子：FNV-1a 64 指纹 → [0.25, 0.95]（同文本恒同；语义打分二期）。
pub fn seed_of(text: &str) -> f32 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mix = (h >> 32) as u32 ^ h as u32;
    let x = mix as f64 / u32::MAX as f64;
    0.25 + 0.70 * x as f32
}

/// 会话：一个条目从点亮到显化的运行状态。
pub struct JournalSession {
    pub entry: ManifestEntry,
    pub engine: LifecycleEngine,
    pub log: Vec<JournalLine>,
    pub tick: u64,
    /// 最近一次低能量意图建议（suggest_next 素材）。
    pub last_intent: Option<String>,
    /// 收敛上限（同 tick 防环）。
    max_events_per_push: usize,
    /// 快照轮询探针（次事件源）。
    probe: Option<SnapshotProbe>,
}

impl JournalSession {
    pub fn new(raw: &str) -> Self {
        let seed = seed_of(raw);
        Self {
            entry: ManifestEntry {
                id: format!("m-{:08x}", (seed * 1e9) as u64 & 0xffff_ffff),
                raw: raw.to_string(),
                seed,
                lifecycle: 0,
                rounds: 0,
                retained_early: 0,
            },
            engine: LifecycleEngine::new(),
            log: Vec::new(),
            tick: 0,
            last_intent: None,
            max_events_per_push: 64,
            probe: None,
        }
    }

    /// 消费一条网关事件（snapshot/ping 为 Hold：无推进、无日志；action 事件推进+记录）。
    /// 返回是否产生日志行。
    pub fn ingest(&mut self, ev: &RawEvent) -> bool {
        // 注意：不去除 Hold 类（snapshot/ping/simulated）——apply(Hold) 无推进副作用，
        // 但允许 99 极显后由非正向事件累积静默窗（回融语义），并允许 simulate 直接驱动。
        self.tick += 1;
        let mut wrote = false;
        // 1) instruction → 陈述（若 Speaker 认识）
        if ev.kind == "instruction" {
            if let Some(st) = Speaker::statement_of_instruction(&ev.data) {
                self.log.push(JournalLine {
                    t: self.tick,
                    lifecycle: self.engine.state,
                    statement: st.text.clone(),
                    source: st.source.clone(),
                });
                wrote = true;
            }
        }
        // 2) lifecycle 推进
        let before = self.engine.state;
        let changed = self.engine.apply(ev.kernel);
        if changed {
            let st = Speaker::statement_of_lifecycle(before, self.engine.state);
            self.log.push(JournalLine {
                t: self.tick,
                lifecycle: self.engine.state,
                statement: st.text.clone(),
                source: st.source,
            });
            wrote = true;
            self.sync_entry();
        } else {
            self.sync_entry();
        }
        // 3) 低能量/回落 → 生成 suggest_next 意图素材（不写入日志，CLI 层展示）
        if ev.kernel == npb_appkit::KernelEvent::Settle {
            let st = Statement {
                text: format!("储备/回落事件：{}", ev.source),
                source: ev.source.clone(),
            };
            self.last_intent = Some(
                Speaker::intent_of(npb_appkit::INTENT_SUGGEST_NEXT, &st, self.engine.state, true).text,
            );
        }
        // 防抖护栏：单事件处理上限
        wrote && self.log.len() < self.max_events_per_push + 128
    }

    fn sync_entry(&mut self) {
        self.entry.lifecycle = self.engine.state;
        self.entry.rounds = self.engine.rounds;
    }

    /// 注入补充扰动：seed 微变体（L5 §5：seed' = clamp(seed+0.05)）→ push。
    pub fn boost_seed(&self) -> f32 {
        (self.entry.seed + 0.05).min(0.95)
    }

    /// 归档（早退回融）：本地 Reset（不杀生：条目保留，状态归锚点，记 retained_early）。
    pub fn archive(&mut self) {
        self.engine.apply(npb_appkit::KernelEvent::Reset);
        self.entry.retained_early += 1;
        self.sync_entry();
    }

    /// 向网关 push 一次（seed 或 boost_seed）。
    pub fn push(&self, addr: &str, seed: f32) -> bool {
        let body = format!(r#"{{"seed":{seed}}}"#);
        httpc::post_json(addr, "/v1/push", &body)
            .map(|r| r.contains("\"accepted\":true"))
            .unwrap_or(false)
    }

    /// 快照轮询（次事件源）：GET /v1/state 与上次比较 → KernelEvent 驱动引擎。
    /// 返回是否产生了推进性变化。真实内核在饱和态 SSE 事件稀疏，轮询保证事件供给。
    pub fn poll_state(&mut self, addr: &str) -> bool {
        let Some(next) = SnapshotProbe::fetch(addr) else { return false };
        let ev = next.event(self.probe.as_ref());
        let fake = RawEvent {
            kind: "state_poll".to_string(),
            data: format!("poll@{next:?}"),
            kernel: ev,
            source: "state_poll#/v1/state".to_string(),
        };
        let before = self.engine.state;
        self.ingest(&fake);
        self.probe = Some(next);
        self.engine.state != before
    }

    /// 端到端一轮：push N 次高活性种子；每推后先消化 SSE 订阅事件，再快照轮询兜底。
    /// 网关 SSE 轮询约 100ms → 每推后消化窗口 ~400ms，空窗 8 次即停。
    pub fn run_pushes(&mut self, addr: &str, pipe_rx: &std::sync::mpsc::Receiver<RawEvent>, n: u32) -> u16 {
        for _ in 0..n {
            let seed = if self.engine.state == 0 { self.entry.seed } else { self.boost_seed() };
            if !self.push(addr, seed) {
                break;
            }
            let mut idle = 0u32;
            for _ in 0..40 {
                match pipe_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                    Ok(ev) => {
                        idle = 0;
                        self.ingest(&ev);
                    }
                    Err(_) => {
                        idle += 1;
                        if idle >= 8 {
                            break;
                        }
                    }
                }
            }
            // 快照轮询兜底（无论 SSE 是否捕到事件）
            let _ = self.poll_state(addr);
        }
        self.engine.state
    }

    /// 模拟事件序列（不走网关）：用于 99→0 回融与确定性重放测试。
    pub fn simulate(&mut self, evs: &[npb_appkit::KernelEvent]) -> u16 {
        for &ev in evs {
            let fake = RawEvent {
                kind: "simulated".to_string(),
                data: String::new(),
                kernel: ev,
                source: "sim#".to_string(),
            };
            self.ingest(&fake);
        }
        self.engine.state
    }
}

/// 简易序列化（JSON-lite 手写；审计/恢复用）。字段顺序固定。
impl ManifestEntry {
    pub fn to_line(&self) -> String {
        format!(
            r#"{{"id":"{}","seed":{:.6},"lifecycle":{},"rounds":{},"retained_early":{},"raw":"{}"}}"#,
            self.id,
            self.seed,
            self.lifecycle,
            self.rounds,
            self.retained_early,
            self.raw.replace('"', "\\\"")
        )
    }

    pub fn from_line(line: &str) -> Option<Self> {
        fn field<'a>(s: &'a str, key: &str) -> Option<String> {
            let needle = format!("\"{key}\"");
            let idx = s.find(&needle)?;
            let rest = &s[idx + needle.len()..];
            let colon = rest.find(':')? + 1;
            let v: String = rest[colon..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '"' || c.is_alphabetic() || *c == ' ' || *c == '\\' || (*c as u32) > 127)
                .collect();
            Some(v.trim_matches('"').to_string())
        }
        let id = field(line, "id")?;
        let seed: f32 = field(line, "seed")?.parse().ok()?;
        let lifecycle: u16 = field(line, "lifecycle")?.parse().ok()?;
        let rounds: u32 = field(line, "rounds")?.parse().ok()?;
        let retained_early: u32 = field(line, "retained_early")?.parse().ok()?;
        let raw = field(line, "raw")?.replace("\\\"", "\"");
        Some(Self { id, raw, seed, lifecycle, rounds, retained_early })
    }
}

/// 状态外部名快捷（日志头/CLI 用）。
pub fn band_name(state: u16) -> String {
    Namer::band_of(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use npb_appkit::KernelEvent;

    #[test]
    fn seed_of_is_deterministic_and_in_range() {
        let t = "把 A4/A5 复核结论写进正式档案";
        let a = seed_of(t);
        let b = seed_of(t);
        assert_eq!(a, b, "同文本恒同种子");
        assert!((0.25..=0.95).contains(&a), "{a}");
        assert!(seed_of("甲") != seed_of("乙"), "不同文本种子不同");
    }

    #[test]
    fn simulate_reaches_99_and_retreats_with_rounds() {
        let mut s = JournalSession::new("示例念头");
        // 点亮 + 冲到 99（每步可能需 1-2 Awaken（跨带），最多 200）
        let mut guard = 0;
        while s.engine.state != 99 && guard < 220 {
            s.simulate(&[KernelEvent::Awaken]);
            guard += 1;
        }
        assert_eq!(s.engine.state, 99);
        // 静默窗 8 个 Hold → 回融 0，轮次 +1
        s.simulate(&[KernelEvent::Hold; 8]);
        assert_eq!(s.engine.state, 0);
        assert_eq!(s.engine.rounds, 1);
        assert_eq!(s.entry.rounds, 1);
        // 日志含回融行（可溯源）
        assert!(s.log.iter().any(|l| l.source.contains("lifecycle")), "日志应含生命周期行");
    }

    #[test]
    fn archive_retreats_early_and_keeps_entry() {
        let mut s = JournalSession::new("早退示例");
        s.simulate(&[KernelEvent::Awaken; 12]);
        assert!(s.engine.state > 10);
        s.archive();
        assert_eq!(s.engine.state, 0);
        assert_eq!(s.entry.retained_early, 1);
        assert_eq!(s.engine.rounds, 0, "归档不计完成轮次");
    }

    #[test]
    fn entry_serialize_roundtrip_preserves_fields() {
        let mut s = JournalSession::new("持久化示例内容");
        s.simulate(&[KernelEvent::Awaken; 15]);
        let line = s.entry.to_line();
        let back = ManifestEntry::from_line(&line).expect("parse");
        assert_eq!(back.id, s.entry.id);
        assert!((back.seed - s.entry.seed).abs() < 1e-5, "seed 往返（6 位小数容差）: {} vs {}", back.seed, s.entry.seed);
        assert_eq!(back.lifecycle, s.entry.lifecycle);
        assert_eq!(back.raw, s.entry.raw, "原文零修改往返");
    }

    #[test]
    fn replay_same_seed_yields_same_trend() {
        // seed 重放：两次从同文本重建并跑同数量 Awaken → 前几步状态序列一致（确定性）
        let t = "确定性重放测试";
        let mut a = JournalSession::new(t);
        let mut b = JournalSession::new(t);
        assert_eq!(a.entry.seed, b.entry.seed);
        let seq_a: Vec<u16> = (0..20).map(|_| a.simulate(&[KernelEvent::Awaken])).collect();
        let mut b_states = Vec::new();
        let mut prev = 0u16;
        for _ in 0..20 {
            prev = b.simulate(&[KernelEvent::Awaken]);
            b_states.push(prev);
        }
        assert_eq!(seq_a, b_states, "重放趋势一致");
    }

    #[test]
    fn log_lines_carry_statement_and_source() {
        let mut s = JournalSession::new("溯源示例");
        s.simulate(&[KernelEvent::Awaken; 3]);
        assert!(!s.log.is_empty());
        for l in s.log.iter().take(5) {
            assert!(!l.statement.is_empty(), "每条日志有陈述");
            assert!(!l.source.is_empty(), "每条日志可溯源");
        }
        // 原文未进入任何日志（零修改展示由 UI 层负责——原始保留在 entry）
        assert_eq!(s.entry.raw, "溯源示例");
    }
}
