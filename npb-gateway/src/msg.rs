//! L6/L7 · 消息收发（宿主侧 HTTP API）。
//!
//! 定位：让「发消息」成为**可离线自洽**的能力——消息由本机网关保存与分发，
//! **不依赖任何外部即时通讯服务**（老笔记本无外网也能用）。
//!
//! - 归属：每条消息带 `from`（`workbuddy` / `kernel` / `self`），与运行日志的
//!   `[WorkBuddy]` / `[元内核]` 前缀体系一致；
//! - 存储：环形缓冲（容量 200），**只在本机内存**；
//! - 安全：只做长度限制与转义（JSON 输出时转义），**不解释内容**。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 消息容量上限。
pub const CAP: usize = 200;
/// 单条消息最大字符数（超出截断，防滥用）。
pub const MAX_LEN: usize = 2000;

/// 一条消息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Msg {
    pub seq: u64,
    pub ts: u64,
    /// 发送方标识（`workbuddy` / `kernel` / `self` / 其它自由短标识）。
    pub from: String,
    pub text: String,
}

impl Msg {
    pub fn json(&self) -> String {
        format!(
            "{{\"seq\":{},\"ts\":{},\"from\":\"{}\",\"text\":\"{}\"}}",
            self.seq,
            self.ts,
            crate::selfmon::json_escape(&self.from),
            crate::selfmon::json_escape(&self.text)
        )
    }
}

/// 消息存储。
pub struct MsgStore {
    inner: Mutex<VecDeque<Msg>>,
    seq: Mutex<u64>,
}

impl Default for MsgStore {
    fn default() -> Self {
        Self::new()
    }
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 发送方归属标签（与日志前缀一致）。
pub fn owner_tag(from: &str) -> &'static str {
    let f = from.to_ascii_lowercase();
    if f.contains("kernel") || f.contains("元内核") {
        "[元内核]"
    } else if f.contains("workbuddy") || f == "wb" {
        "[WorkBuddy]"
    } else {
        "[本机]"
    }
}

impl MsgStore {
    pub fn new() -> Self {
        Self { inner: Mutex::new(VecDeque::with_capacity(CAP)), seq: Mutex::new(0) }
    }

    /// 追加一条消息（自动截断超长、限制容量）。返回消息序号。
    pub fn push(&self, from: &str, text: &str) -> u64 {
        let f = if from.trim().is_empty() { "self".to_string() } else { from.trim().to_string() };
        let f = if f.len() > 32 { f[..32].to_string() } else { f };
        let mut t = text.to_string();
        if t.chars().count() > MAX_LEN {
            t = t.chars().take(MAX_LEN).collect();
        }
        let seq = {
            let mut s = match self.seq.lock() {
                Ok(s) => s,
                Err(p) => p.into_inner(),
            };
            *s += 1;
            *s
        };
        let m = Msg { seq, ts: now_s(), from: f, text: t };
        let mut g = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if g.len() >= CAP {
            g.pop_front();
        }
        g.push_back(m);
        seq
    }

    /// 最近 n 条（时间正序）。
    pub fn recent(&self, n: usize) -> Vec<Msg> {
        let g = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let skip = g.len().saturating_sub(n);
        g.iter().skip(skip).cloned().collect()
    }

    pub fn len(&self) -> usize {
        match self.inner.lock() {
            Ok(g) => g.len(),
            Err(p) => p.into_inner().len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// JSON 数组（含归属标签，前端直接显示）。
    pub fn json(&self, n: usize) -> String {
        let ms = self.recent(n);
        let body = ms
            .iter()
            .map(|m| {
                format!(
                    "{{\"seq\":{},\"ts\":{},\"from\":\"{}\",\"owner\":\"{}\",\"text\":\"{}\"}}",
                    m.seq,
                    m.ts,
                    crate::selfmon::json_escape(&m.from),
                    owner_tag(&m.from),
                    crate::selfmon::json_escape(&m.text)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"schema\":1,\"count\":{},\"capacity\":{},\"msgs\":[{}]}}", ms.len(), CAP, body)
    }

    /// 纯文本（每行带归属前缀）。
    pub fn text(&self, n: usize) -> String {
        let mut s = String::new();
        for m in self.recent(n) {
            s.push_str(&format!("{} {} · {}\n", owner_tag(&m.from), m.from, m.text));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_recent_roundtrip() {
        let s = MsgStore::new();
        assert!(s.is_empty());
        let a = s.push("workbuddy", "第一条");
        let b = s.push("kernel", "第二条");
        assert!(b > a);
        assert_eq!(s.len(), 2);
        let r = s.recent(10);
        assert_eq!(r[0].text, "第一条");
        assert_eq!(r[1].from, "kernel");
        assert!(r[0].ts > 0);
    }

    #[test]
    fn owner_tags_match_log_prefixes() {
        assert_eq!(owner_tag("workbuddy"), "[WorkBuddy]");
        assert_eq!(owner_tag("Kernel"), "[元内核]");
        assert_eq!(owner_tag("self"), "[本机]");
        assert_eq!(owner_tag(""), "[本机]");
    }

    #[test]
    fn capacity_is_enforced_and_latest_kept() {
        let s = MsgStore::new();
        for i in 0..(CAP + 20) {
            s.push("self", &format!("m{i}"));
        }
        assert_eq!(s.len(), CAP, "容量上限生效");
        let last = s.recent(1);
        assert_eq!(last[0].text, format!("m{}", CAP + 19), "保留最新");
    }

    #[test]
    fn long_text_is_truncated_and_empty_from_defaults() {
        let s = MsgStore::new();
        let long = "x".repeat(MAX_LEN + 500);
        s.push("   ", &long);
        let m = &s.recent(1)[0];
        assert_eq!(m.from, "self", "空发送方 → self");
        assert_eq!(m.text.chars().count(), MAX_LEN, "超长被截断");
    }

    #[test]
    fn json_escapes_quotes_and_newlines() {
        let s = MsgStore::new();
        s.push("workbuddy", "含\"引号\"与\n换行");
        let j = s.json(10);
        assert!(j.contains("\\\""), "{j}");
        assert!(j.contains("\\n"), "{j}");
        assert!(j.contains("\"owner\":\"[WorkBuddy]\""), "{j}");
    }

    #[test]
    fn text_output_has_prefix_per_line() {
        let s = MsgStore::new();
        s.push("workbuddy", "甲");
        s.push("kernel", "乙");
        let t = s.text(10);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("[WorkBuddy]"), "{}", lines[0]);
        assert!(lines[1].starts_with("[元内核]"), "{}", lines[1]);
    }
}
