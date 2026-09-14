//! 元内核 · 自我监控 / 健康报告 / 异常告警 / 运行日志（含**任务归属**）。
//!
//! 设计（v0.106，发起人指令「元内核系统接管更多任务 · 立即移交」）：
//! - **任务归属**：每行日志带前缀 —— `[元内核]`（内核自身产生）／`[WorkBuddy]`（执行者/协作方产生）。
//! - **运行日志**：环形缓冲（容量 512），每条含 序号 / 时间 / 归属 / 级别 / 事件 / 明细。
//! - **自我监控**：进程启动时刻 + 计数器（请求 / 错误 / 探针 / 推注 / 拒绝 / 任务投递）。
//! - **健康报告**：运行时长 + 计数器 + 告警数 + 最新日志头（JSON，供工作台/告警面板读取）。
//! - **异常告警**：由规则派生（错误率偏高 / 久无探针 / 拒绝激增），不引入外部依赖。
//!
//! 零依赖（std only）；线程安全（Mutex）。**只观测、不干预**（不参与判定，守 L5「只读」精神）。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 归属前缀：内核自身。
pub const OWNER_KERNEL: &str = "[元内核]";
/// 归属前缀：执行者 / 协作方。
pub const OWNER_WORKBUDDY: &str = "[WorkBuddy]";

/// 日志容量上限。
pub const LOG_CAP: usize = 512;

/// 任务归属。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// 元内核系统自身产生（探针接入、内核推注、拒绝等）。
    Kernel,
    /// WorkBuddy（执行者/协作方）投递的任务与动作。
    WorkBuddy,
}

impl Owner {
    pub fn tag(&self) -> &'static str {
        match self {
            Owner::Kernel => OWNER_KERNEL,
            Owner::WorkBuddy => OWNER_WORKBUDDY,
        }
    }
    /// 从文本解析（`workbuddy` / `wb` / 其它 → Kernel）。
    pub fn parse(s: &str) -> Owner {
        let t = s.to_ascii_lowercase();
        if t.contains("workbuddy") || t == "wb" || t.contains("work buddy") {
            Owner::WorkBuddy
        } else {
            Owner::Kernel
        }
    }
}

/// 一条运行日志。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub seq: u64,
    /// Unix 秒（避免引入时间库依赖；UI 侧格式化）。
    pub ts: u64,
    pub owner: Owner,
    pub level: &'static str,
    pub event: &'static str,
    pub detail: String,
}

impl LogLine {
    /// 单行文本（**归属前缀在最前**）：`[元内核] INFO 场域探针上报 · len=231`
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str(self.owner.tag());
        s.push(' ');
        s.push_str(self.level);
        s.push(' ');
        s.push_str(self.event);
        if !self.detail.is_empty() {
            s.push_str(" · ");
            s.push_str(&self.detail);
        }
        s
    }
    /// JSON 片段（零依赖手写）。
    pub fn json(&self) -> String {
        format!(
            "{{\"seq\":{},\"ts\":{},\"owner\":\"{}\",\"level\":\"{}\",\"event\":\"{}\",\"detail\":\"{}\"}}",
            self.seq,
            self.ts,
            self.owner.tag(),
            self.level,
            self.event,
            json_escape(&self.detail)
        )
    }
}

/// 计数器（自我监控）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub requests: u64,
    pub errors: u64,
    pub probes: u64,
    pub pushes: u64,
    pub rejected: u64,
    pub task_posts: u64,
}

/// 告警。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub level: &'static str,
    pub code: &'static str,
    pub message: String,
}

struct Inner {
    seq: u64,
    lines: VecDeque<LogLine>,
    c: Counters,
}

/// 自我监控器。
pub struct SelfMon {
    start: u64,
    inner: Mutex<Inner>,
}

impl Default for SelfMon {
    fn default() -> Self {
        Self::new()
    }
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

impl SelfMon {
    pub fn new() -> Self {
        Self { start: now_s(), inner: Mutex::new(Inner { seq: 0, lines: VecDeque::with_capacity(LOG_CAP), c: Counters::default() }) }
    }

    /// 记一条日志（自动补序号与时间；超出容量丢最旧）。
    pub fn note(&self, owner: Owner, level: &'static str, event: &'static str, detail: impl Into<String>) -> LogLine {
        let mut g = match self.inner.lock() { Ok(g) => g, Err(p) => p.into_inner() };
        g.seq += 1;
        let line = LogLine { seq: g.seq, ts: now_s(), owner, level, event, detail: detail.into() };
        if g.lines.len() >= LOG_CAP {
            g.lines.pop_front();
        }
        g.lines.push_back(line.clone());
        line
    }

    /// 计数器自增（用闭包挑字段）。
    pub fn bump(&self, f: impl FnOnce(&mut Counters)) {
        let mut g = match self.inner.lock() { Ok(g) => g, Err(p) => p.into_inner() };
        f(&mut g.c);
    }

    pub fn count_request(&self) { self.bump(|c| c.requests += 1); }
    pub fn count_error(&self) { self.bump(|c| c.errors += 1); }
    /// 探针上报计数（请求总数由分发入口统一计，避免重复）。
    pub fn count_probe(&self) { self.bump(|c| c.probes += 1); }
    pub fn count_push(&self, ok: bool) {
        self.bump(|c| { c.pushes += 1; if !ok { c.rejected += 1; } });
    }
    pub fn count_task_post(&self) { self.bump(|c| c.task_posts += 1); }

    pub fn counters(&self) -> Counters {
        match self.inner.lock() { Ok(g) => g.c, Err(p) => p.into_inner().c }
    }

    /// 运行时长（秒）。
    pub fn uptime_s(&self) -> u64 {
        now_s().saturating_sub(self.start)
    }

    /// 最近 n 条日志（按时间正序）。
    pub fn lines(&self, n: usize) -> Vec<LogLine> {
        let g = match self.inner.lock() { Ok(g) => g, Err(p) => p.into_inner() };
        let skip = g.lines.len().saturating_sub(n);
        g.lines.iter().skip(skip).cloned().collect()
    }

    pub fn log_len(&self) -> usize {
        match self.inner.lock() { Ok(g) => g.lines.len(), Err(p) => p.into_inner().lines.len() }
    }

    /// 运行日志纯文本（每行带归属前缀）。
    pub fn tasks_txt(&self, n: usize) -> String {
        let mut s = String::new();
        for l in self.lines(n) {
            s.push_str(&l.text());
            s.push('\n');
        }
        s
    }

    /// 运行日志 JSON 数组。
    pub fn tasks_json(&self, n: usize) -> String {
        let ls = self.lines(n);
        let body = ls.iter().map(|l| l.json()).collect::<Vec<_>>().join(",");
        format!("{{\"schema\":1,\"count\":{},\"kernels\":\"{}\",\"logs\":[{}]}}", ls.len(), OWNER_KERNEL, body)
    }

    /// **异常告警**（规则派生；只读，不改系统）。
    pub fn alerts(&self) -> Vec<Alert> {
        let c = self.counters();
        let up = self.uptime_s();
        let mut out = Vec::new();
        // ① 错误率偏高
        if c.errors > 0 && c.requests >= 10 && c.errors * 10 > c.requests {
            out.push(Alert {
                level: "WARN",
                code: "ERR_RATE_HIGH",
                message: format!("错误率偏高：{}/{} 次请求", c.errors, c.requests),
            });
        }
        // ② 久无探针（上线 > 5 分钟且一次都没收到）
        if c.probes == 0 && up > 300 {
            out.push(Alert {
                level: "WARN",
                code: "NO_PROBE",
                message: format!("已运行 {} 秒仍无场域探针上报（目标设备未接探针？）", up),
            });
        }
        // ③ 推注被拒比例偏高（闸门在起作用，但比例异常需人工看一眼）
        if c.pushes >= 10 && c.rejected * 10 > c.pushes * 6 {
            out.push(Alert {
                level: "INFO",
                code: "REJECT_RATE_HIGH",
                message: format!("推注被拒比例偏高：{}/{}（闸门正常但需复核输入源）", c.rejected, c.pushes),
            });
        }
        out
    }

    /// 告警 JSON。
    pub fn alerts_json(&self) -> String {
        let a = self.alerts();
        let body = a.iter().map(|x| format!(
            "{{\"level\":\"{}\",\"code\":\"{}\",\"message\":\"{}\"}}", x.level, x.code, json_escape(&x.message)
        )).collect::<Vec<_>>().join(",");
        format!("{{\"schema\":1,\"count\":{},\"alerts\":[{}]}}", a.len(), body)
    }

    /// **健康报告**（自我监控汇总；JSON）。
    /// `version` = 组件版本；`features` = 能力特征（逗号分隔，**版本无关的升级判据**）。
    pub fn report_json(&self, version: &str, features: &str) -> String {
        let c = self.counters();
        let a = self.alerts();
        let last = self.lines(3);
        let last_txt = last.iter().map(|l| json_escape(&l.text())).collect::<Vec<_>>().join("|");
        format!(
            "{{\"schema\":1,\"ok\":{},\"project\":\"云内核项目\",\"component\":\"元内核系统\",\"version\":\"{}\",\"features\":\"{}\",\"uptime_s\":{},\"counters\":{{\"requests\":{},\"errors\":{},\"probes\":{},\"pushes\":{},\"rejected\":{},\"task_posts\":{}}},\"log_lines\":{},\"log_cap\":{},\"alerts\":{},\"last\":\"{}\"}}",
            if a.iter().all(|x| x.level != "WARN") { "true" } else { "false" },
            version,
            features,
            self.uptime_s(),
            c.requests, c.errors, c.probes, c.pushes, c.rejected, c.task_posts,
            self.log_len(), LOG_CAP, a.len(), last_txt
        )
    }

    /// 健康报告纯文本（供人读）。
    pub fn report_txt(&self, version: &str, features: &str) -> String {
        let c = self.counters();
        let mut s = String::new();
        s.push_str(&format!("{} 健康报告 · 云内核项目 / 元内核系统\n", OWNER_KERNEL));
        s.push_str(&format!("组件版本 : {}\n", version));
        s.push_str(&format!("能力特征 : {}\n", features));
        s.push_str(&format!("运行时长 : {} 秒\n", self.uptime_s()));
        s.push_str(&format!("请求/错误 : {} / {}\n", c.requests, c.errors));
        s.push_str(&format!("探针/推注 : {} / {}（被拒 {}）\n", c.probes, c.pushes, c.rejected));
        s.push_str(&format!("任务投递 : {}\n", c.task_posts));
        s.push_str(&format!("日志条数 : {}（上限 {}）\n", self.log_len(), LOG_CAP));
        let a = self.alerts();
        s.push_str(&format!("告警     : {}\n", a.len()));
        for x in a {
            s.push_str(&format!("  - {} [{}] {}\n", x.level, x.code, x.message));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_tags_are_the_required_prefixes() {
        assert_eq!(Owner::Kernel.tag(), "[元内核]");
        assert_eq!(Owner::WorkBuddy.tag(), "[WorkBuddy]");
    }

    #[test]
    fn log_lines_carry_owner_prefix() {
        let m = SelfMon::new();
        let a = m.note(Owner::Kernel, "INFO", "场域探针上报", "len=231");
        let b = m.note(Owner::WorkBuddy, "TASK", "升级老笔记本", "v0.105");
        assert!(a.text().starts_with("[元内核] INFO 场域探针上报"), "{}", a.text());
        assert!(b.text().starts_with("[WorkBuddy] TASK 升级老笔记本"), "{}", b.text());
        assert_eq!(a.seq, 1);
        assert_eq!(b.seq, 2);
    }

    #[test]
    fn ring_buffer_caps_and_keeps_latest() {
        let m = SelfMon::new();
        for i in 0..(LOG_CAP + 10) {
            m.note(Owner::Kernel, "INFO", "e", i.to_string());
        }
        assert_eq!(m.log_len(), LOG_CAP, "容量上限生效");
        let last = m.lines(1);
        assert_eq!(last[0].detail, (LOG_CAP + 9).to_string(), "保留最新");
    }

    #[test]
    fn counters_and_report_json_shape() {
        let m = SelfMon::new();
        m.count_request();
        m.count_probe();
        m.count_push(true);
        m.count_push(false);
        let c = m.counters();
        assert_eq!(c.requests, 1, "普通请求计 1 次（探针的请求数由分发入口统一计）");
        assert_eq!(c.probes, 1);
        assert_eq!(c.pushes, 2);
        assert_eq!(c.rejected, 1);
        let j = m.report_json("0.106", "report,alerts,tasks,upgrade");
        assert!(j.contains("\"version\":\"0.106\""));
        assert!(j.contains("\"features\":\"report,alerts,tasks,upgrade\""));
        assert!(j.contains("\"probes\":1"));
        assert!(j.contains("\"rejected\":1"));
        assert!(j.contains("\"log_cap\":512"));
    }

    #[test]
    fn alerts_fire_on_expected_rules() {
        let m = SelfMon::new();
        // 无探针 + 上线时间不足 → 不告警
        assert!(m.alerts().is_empty(), "刚启动不应告警");
        // 造出高错误率
        for _ in 0..12 { m.count_request(); }
        for _ in 0..11 { m.count_error(); }
        let a = m.alerts();
        assert!(a.iter().any(|x| x.code == "ERR_RATE_HIGH"), "{:?}", a);
    }

    #[test]
    fn tasks_txt_every_line_has_prefix() {
        let m = SelfMon::new();
        m.note(Owner::Kernel, "INFO", "内核推注", "seed=0.5");
        m.note(Owner::WorkBuddy, "TASK", "网络排查", "net-check-one.bat");
        let t = m.tasks_txt(10);
        for line in t.lines() {
            assert!(line.starts_with("[元内核]") || line.starts_with("[WorkBuddy]"), "缺少归属前缀: {line}");
        }
        assert_eq!(t.lines().count(), 2);
    }

    #[test]
    fn json_escape_handles_quotes_and_control() {
        assert_eq!(json_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        let m = SelfMon::new();
        let l = m.note(Owner::Kernel, "INFO", "test", "含\"引号\"与\\反斜杠");
        let j = l.json();
        assert!(j.contains("\\\""), "{j}");
    }
}
