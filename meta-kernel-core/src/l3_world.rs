//! L3 · **世界模型**（借鉴 Visionary 的"载体"定位）
//!
//! 定位（发起人 v0.122）：空天浏览器不只渲染场域，**还承载场域的世界模型**。
//! 用户的每一次交互（取源码 / 切标签 / 调四元组）都在**更新世界模型**；
//! 画面呈现的是**当前世界模型的状态**（页面场域与其累积状态合成）。
//!
//! ## 纪律
//! - **零依赖**（内核红线）：只用 `String` / `Vec`。
//! - **可查询**：`summary()` 给出当前世界状态；`entries()` 给出全部条目。
//! - **可更新**：`observe_page` / `observe_interaction` 累积。
//! - **可持久化**：`to_text` / `from_text`（与基因库同模式，宿主只存取原文）。
//! - **可复现**：同输入序列 → 同模型（权重用确定性加权平均，不用随机）。

use crate::l1_field_parse::FieldReading;
use crate::l5_quad::Quad;

/// 世界模型中的条目类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldKind {
    /// 一个具体页面（host + 路径）。
    Page,
    /// 一个站点（host 粒度，聚合其下页面）。
    Host,
    /// 一次交互（调四元组 / 切标签等）——交互本身也是世界的一部分。
    Interaction,
}

impl WorldKind {
    fn tag(self) -> &'static str {
        match self {
            WorldKind::Page => "page",
            WorldKind::Host => "host",
            WorldKind::Interaction => "interaction",
        }
    }
    fn from_tag(s: &str) -> Option<Self> {
        match s {
            "page" => Some(WorldKind::Page),
            "host" => Some(WorldKind::Host),
            "interaction" => Some(WorldKind::Interaction),
            _ => None,
        }
    }
}

/// 世界模型条目：一个"被观察到的存在"及其累积状态。
#[derive(Debug, Clone, PartialEq)]
pub struct WorldEntry {
    pub key: String,
    pub kind: WorldKind,
    /// 被观察到的次数。
    pub hits: u32,
    /// 累积权重（默认每次 +1.0；用于加权平均）。
    pub weight: f64,
    /// 四场累积均值（该存在"长什么样"）。
    pub mean: [f64; 4],
    /// 最近一次被更新的 tick。
    pub last_tick: u64,
}

impl WorldEntry {
    fn new(key: String, kind: WorldKind, tick: u64) -> Self {
        Self { key, kind, hits: 0, weight: 0.0, mean: [0.0; 4], last_tick: tick }
    }

    /// 用一次观察更新均值（**确定性**加权平均；`w` 为本次观察权重）。
    fn fold(&mut self, field: &FieldReading, w: f64, tick: u64) {
        let w = if w.is_finite() && w > 0.0 { w } else { 1.0 };
        let total = self.weight + w;
        for i in 0..4 {
            self.mean[i] = (self.mean[i] * self.weight + field.to_array()[i] * w) / total;
        }
        self.weight = total;
        self.hits = self.hits.saturating_add(1);
        self.last_tick = tick;
    }
}

/// 世界模型（累积状态）。
#[derive(Debug, Clone, Default)]
pub struct WorldModel {
    tick: u64,
    entries: Vec<WorldEntry>,
}

/// 世界状态摘要（**画面参数由它驱动**）。
#[derive(Debug, Clone, PartialEq)]
pub struct WorldSummary {
    /// 条目总数。
    pub entries: usize,
    /// 当前 tick。
    pub tick: u64,
    /// 全部条目的加权平均四场（"世界的当前样子"）。
    pub mean: [f64; 4],
    /// 世界的内在稳定度 0..1（由四元组/均值推出）——画面"连贯度"的来源之一。
    pub coherence: f64,
    /// 最"重"的条目 key（无条目时为空串）。
    pub dominant: String,
}

impl WorldModel {
    pub fn new() -> Self {
        Self { tick: 0, entries: Vec::new() }
    }

    pub fn entries(&self) -> &[WorldEntry] {
        &self.entries
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 推进一个时间步（每次交互/取源码都算一步）。
    pub fn advance(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    fn upsert(&mut self, key: &str, kind: WorldKind) -> usize {
        let tick = self.tick;
        if let Some(i) = self.entries.iter().position(|e| e.key == key && e.kind == kind) {
            return i;
        }
        self.entries.push(WorldEntry::new(key.to_string(), kind, tick));
        self.entries.len() - 1
    }

    /// **更新**：观察到一个页面（页面条目 + 其 host 聚合条目都会被更新）。
    /// 返回 (page_index, host_index)。
    pub fn observe_page(&mut self, host: &str, url: &str, field: &FieldReading, chars: usize) -> (usize, usize) {
        self.advance();
        // 观察权重：内容越多越"实"（但设上限，避免超长页面压制其它观察）
        let w = (0.5 + (chars as f64 / 2000.0)).min(3.0);
        let pi = self.upsert(url, WorldKind::Page);
        self.entries[pi].fold(field, w, self.tick);
        let hi = self.upsert(host, WorldKind::Host);
        self.entries[hi].fold(field, w, self.tick);
        (pi, hi)
    }

    /// **更新**：观察到一个页面（不带 url 时以 host 作页面键）。
    pub fn observe_source(&mut self, host: &str, field: &FieldReading, chars: usize) -> (usize, usize) {
        self.observe_page(host, host, field, chars)
    }

    /// **更新**：观察一次交互（调四元组 / 切标签）。交互以"四元组状态"折算成一个虚拟场域，
    /// 使其也能参与世界均值（这样"用户的内在状态"确实是世界的一部分）。
    pub fn observe_interaction(&mut self, label: &str, quad: &Quad) {
        self.advance();
        let f = quad_as_field(quad);
        let key = format!("act:{label}");
        let i = self.upsert(&key, WorldKind::Interaction);
        self.entries[i].fold(&f, 1.0, self.tick);
    }

    /// **查询**：世界状态摘要。
    pub fn summary(&self) -> WorldSummary {
        let mut sum = [0.0f64; 4];
        let mut wsum = 0.0f64;
        let mut dominant = String::new();
        let mut dw = -1.0f64;
        for e in &self.entries {
            if e.weight <= 0.0 {
                continue;
            }
            for i in 0..4 {
                sum[i] += e.mean[i] * e.weight;
            }
            wsum += e.weight;
            if e.weight > dw {
                dw = e.weight;
                dominant = e.key.clone();
            }
        }
        let mean = if wsum > 0.0 {
            [sum[0] / wsum, sum[1] / wsum, sum[2] / wsum, sum[3] / wsum]
        } else {
            [0.0; 4]
        };
        // 世界连贯度：结构度越高、交互越少（越安定）→ 越连贯
        let coherence = if wsum > 0.0 {
            (0.35 + 0.5 * mean[0] - 0.25 * mean[3]).clamp(0.0, 1.0)
        } else {
            0.0
        };
        WorldSummary {
            entries: self.entries.len(),
            tick: self.tick,
            mean,
            coherence,
            dominant,
        }
    }

    /// **持久化**：文本快照（宿主只存取原文，不解释）。
    pub fn to_text(&self) -> String {
        let mut s = format!("world v1 tick={}\n", self.tick);
        for e in &self.entries {
            s.push_str(&format!(
                "{}\t{}\t{}\t{:.6}\t{:.6},{:.6},{:.6},{:.6}\t{}\n",
                e.key, e.kind.tag(), e.hits, e.weight,
                e.mean[0], e.mean[1], e.mean[2], e.mean[3], e.last_tick
            ));
        }
        s
    }

    /// **持久化**：从文本恢复；返回成功载入的条目数。
    pub fn from_text(&mut self, text: &str) -> usize {
        let mut loaded = 0usize;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("world v1 tick=") {
                self.tick = rest.trim().parse().unwrap_or(self.tick);
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() != 6 {
                continue;
            }
            let Some(kind) = WorldKind::from_tag(f[1]) else { continue };
            let hits: u32 = f[2].parse().unwrap_or(0);
            let weight: f64 = f[3].parse().unwrap_or(0.0);
            let mean: Vec<f64> = f[4].split(',').filter_map(|x| x.parse().ok()).collect();
            if mean.len() != 4 {
                continue;
            }
            let last_tick: u64 = f[5].parse().unwrap_or(0);
            self.entries.push(WorldEntry {
                key: f[0].to_string(),
                kind,
                hits,
                weight,
                mean: [mean[0], mean[1], mean[2], mean[3]],
                last_tick,
            });
            loaded += 1;
        }
        loaded
    }
}

/// 四元组 → 虚拟场域（让"内在状态"也能作为世界的一部分被累积）。
fn quad_as_field(q: &Quad) -> FieldReading {
    let tension = q.tension.clamp(0.0, 1.0);
    let calm = q.calm.clamp(0.0, 1.0);
    let liking = q.liking.clamp(0.0, 1.0);
    let safety = q.safety.clamp(0.0, 1.0);
    FieldReading {
        earth: safety,                                   // 安全 = 结构稳
        water: calm,                                     // 平静 = 内容沉淀
        fire: tension,                                   // 紧张 = 张力/活跃
        wind: (1.0 - calm) * 0.5 + liking * 0.5,         // 不平静 + 喜欢 = 流动
        confidence: ((calm + safety) * 0.5).clamp(0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(e: f64, w: f64, fi: f64, wi: f64) -> FieldReading {
        FieldReading { earth: e, water: w, fire: fi, wind: wi, confidence: 0.8 }
    }

    #[test]
    fn observe_accumulates_and_is_queryable() {
        let mut m = WorldModel::new();
        m.observe_page("example.com", "example.com/a", &f(0.2, 0.8, 0.1, 0.1), 1000);
        m.observe_page("example.com", "example.com/a", &f(0.4, 0.6, 0.3, 0.1), 1000);
        let s = m.summary();
        assert_eq!(s.entries, 2, "页面 + host 两条");
        assert_eq!(s.tick, 2);
        // 两次均值：(0.2+0.4)/2 = 0.3
        assert!((s.mean[0] - 0.3).abs() < 1e-9, "earth 均值 {}", s.mean[0]);
        let page = m.entries().iter().find(|e| e.kind == WorldKind::Page).unwrap();
        assert_eq!(page.hits, 2);
    }

    #[test]
    fn interaction_updates_world() {
        let mut m = WorldModel::new();
        m.observe_interaction("quad", &Quad { tension: 1.0, calm: 0.0, liking: 0.5, safety: 0.5 });
        let s = m.summary();
        assert_eq!(s.entries, 1);
        assert!(s.mean[2] > 0.9, "紧张应进入世界均值 fire={}", s.mean[2]);
    }

    #[test]
    fn same_input_same_model() {
        let mut a = WorldModel::new();
        let mut b = WorldModel::new();
        for m in [&mut a, &mut b] {
            m.observe_page("h", "h/p", &f(0.3, 0.5, 0.2, 0.4), 500);
            m.observe_interaction("quad", &Quad { tension: 0.3, calm: 0.7, liking: 0.4, safety: 0.6 });
        }
        assert_eq!(a.to_text(), b.to_text(), "同输入序列必须得到同模型（可复现）");
    }

    #[test]
    fn text_roundtrip() {
        let mut m = WorldModel::new();
        m.observe_page("h1", "h1/a", &f(0.25, 0.55, 0.35, 0.45), 800);
        m.observe_interaction("tab2", &Quad { tension: 0.2, calm: 0.8, liking: 0.6, safety: 0.7 });
        let txt = m.to_text();
        let mut back = WorldModel::new();
        let n = back.from_text(&txt);
        assert_eq!(n, m.entries().len());
        assert_eq!(back.to_text(), txt, "往返必须一致");
    }

    #[test]
    fn summary_is_bounded_and_coherence_moves() {
        let mut calm = WorldModel::new();
        calm.observe_page("h", "h/p", &f(0.9, 0.9, 0.1, 0.05), 100);
        let mut wild = WorldModel::new();
        wild.observe_page("h", "h/p", &f(0.05, 0.1, 0.9, 0.95), 100);
        let (a, b) = (calm.summary(), wild.summary());
        assert!(a.coherence > b.coherence, "结构高、张力低的世界更连贯：{} vs {}", a.coherence, b.coherence);
        for v in a.mean.iter().chain(b.mean.iter()) {
            assert!((0.0..=1.0).contains(v), "均值应界定在 0..1：{v}");
        }
    }

    #[test]
    fn from_text_tolerates_garbage() {
        let mut m = WorldModel::new();
        let n = m.from_text("world v1 tick=3\nbadline\nx\tpage\t1\t1.0\t1,2,3\n");
        assert_eq!(n, 0, "畸形行应被跳过而不是 panic");
        assert_eq!(m.tick(), 3, "合法头行仍应生效");
    }
}
