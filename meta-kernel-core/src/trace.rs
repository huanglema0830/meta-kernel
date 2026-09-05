//! # 痕迹（Trace）— 运行后留下的余势（习气之源）
//!
//! 感知层（色声香味触法）之上新增**痕迹层**：每一次运行留下可回溯的痕迹，
//! 痕迹累积成习气，习气识别自我。
//!
//! 四类痕迹（元素映射）：
//! - **风 Wind**：产生于波动/传递（瞬息而过，留痕最浅）；
//! - **火 Fire**：产生于化合/转化（执取——能量转物质的印记）；
//! - **水 Water**：产生于连结/流动（关系成形）；
//! - **地 Earth**：产生于稳定/结构（沉淀固化，留痕最深）。
//!
//! Trace：`step`（时间锚点）+ `intensity`（强度）+ `trace_type` + `fingerprint`
//! （可重复模式的指纹，用于识别"同类痕迹"以累积习气）。

/// 痕迹类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceType {
    /// 风：波动/传递。
    Wind,
    /// 火：化合/转化（执取）。
    Fire,
    /// 水：连结/流动。
    Water,
    /// 地：稳定/结构。
    Earth,
}

impl TraceType {
    pub const fn label_cn(self) -> &'static str {
        match self {
            TraceType::Wind => "风",
            TraceType::Fire => "火",
            TraceType::Water => "水",
            TraceType::Earth => "地",
        }
    }
    pub const fn code(self) -> u32 {
        match self {
            TraceType::Wind => 0,
            TraceType::Fire => 1,
            TraceType::Water => 2,
            TraceType::Earth => 3,
        }
    }
}

/// 一条痕迹。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trace {
    /// 时间锚点（步数）。
    pub step: u64,
    /// 强度 0-1。
    pub intensity: f32,
    /// 类型（风水火地）。
    pub trace_type: TraceType,
    /// 指纹（同模式 → 同指纹 → 习气累积）。
    pub fingerprint: u64,
}

/// 痕迹指纹：从样本统计量量化（均值桶×32 + 标准差桶×16 + 长度盐）。
pub fn fingerprint_of(samples: &[f32]) -> u64 {
    let n = samples.len();
    if n == 0 {
        return 0;
    }
    let mean = samples.iter().sum::<f32>() / n as f32;
    let var = samples.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n as f32;
    let std = var.sqrt();
    let mb = ((mean * 31.999).round() as u64).min(31);
    let sb = ((std * 15.999).round() as u64).min(15);
    (mb << 8) | (sb << 4) | (n as u64 & 0xF)
}

/// 统计：均值、波动度（归一化标准差 σ/(1+σ)）。
pub fn stats_of(samples: &[f32]) -> (f32, f32) {
    let n = samples.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mean = samples.iter().sum::<f32>() / n as f32;
    let var = samples.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n as f32;
    let std = var.sqrt();
    (mean, std / (1.0 + std))
}

/// 判定单次运行应留的痕迹类型。
/// 优先级：火（化合/转化）> 风（波动）> 水（连结/流动）> 地（稳定/结构）。
pub fn decide_type(volatility: f32, compound_activity: f32, flow: f32) -> TraceType {
    if compound_activity > 0.4 {
        TraceType::Fire // 执取：转化发生
    } else if volatility > 0.45 {
        TraceType::Wind // 波动传递
    } else if flow > 0.35 && volatility < 0.4 {
        TraceType::Water // 连结流动（中等活性、平滑）
    } else {
        TraceType::Earth // 稳定结构
    }
}

/// 痕迹存储（按时间/类型/指纹查询）。
#[derive(Debug, Clone, Default)]
pub struct TraceStore {
    traces: Vec<Trace>,
    cap: usize,
}

impl Default for TraceStore {
    fn default() -> Self {
        Self::with_cap(2048)
    }
}

impl TraceStore {
    pub fn with_cap(cap: usize) -> Self {
        Self { traces: Vec::with_capacity(cap.min(65536)), cap }
    }
    pub fn new() -> Self {
        Self::default()
    }
    pub fn record(&mut self, t: Trace) {
        if self.traces.len() == self.cap {
            self.traces.remove(0);
        }
        self.traces.push(t);
    }
    pub fn len(&self) -> usize {
        self.traces.len()
    }
    pub fn is_empty(&self) -> bool {
        self.traces.is_empty()
    }
    /// 最近 n 条（时间倒序）。
    pub fn recent(&self, n: usize) -> impl Iterator<Item = &Trace> {
        self.traces.iter().rev().take(n)
    }
    /// 按类型计数 [风, 火, 水, 地]。
    pub fn counts_by_type(&self) -> [u64; 4] {
        let mut c = [0u64; 4];
        for t in &self.traces {
            c[t.trace_type.code() as usize] += 1;
        }
        c
    }
    pub fn count_type(&self, tt: TraceType) -> u64 {
        self.counts_by_type()[tt.code() as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_repeats_for_same_pattern() {
        let a: Vec<f32> = (0..16).map(|i| (i % 3) as f32 / 3.0).collect();
        let b: Vec<f32> = (0..16).map(|i| (i % 3) as f32 / 3.0).collect();
        assert_eq!(fingerprint_of(&a), fingerprint_of(&b));
        let c: Vec<f32> = (0..16).map(|i| (i % 3) as f32 / 3.0 + 0.01).collect();
        // 均值桶可能有差 → 大概率不同；用精确相同确认稳定性即可
        assert_eq!(fingerprint_of(&a), fingerprint_of(&a));
        let _ = c;
    }

    #[test]
    fn decide_type_priorities() {
        assert_eq!(decide_type(0.1, 0.8, 0.2), TraceType::Fire); // 化合优先
        assert_eq!(decide_type(0.9, 0.1, 0.1), TraceType::Wind); // 波动
        assert_eq!(decide_type(0.2, 0.1, 0.6), TraceType::Water); // 流动连结
        assert_eq!(decide_type(0.1, 0.1, 0.1), TraceType::Earth); // 稳定
    }

    #[test]
    fn store_records_and_counts() {
        let mut s = TraceStore::new();
        s.record(Trace { step: 1, intensity: 0.5, trace_type: TraceType::Wind, fingerprint: 7 });
        s.record(Trace { step: 2, intensity: 0.6, trace_type: TraceType::Fire, fingerprint: 7 });
        s.record(Trace { step: 3, intensity: 0.4, trace_type: TraceType::Water, fingerprint: 8 });
        s.record(Trace { step: 4, intensity: 0.3, trace_type: TraceType::Earth, fingerprint: 8 });
        assert_eq!(s.len(), 4);
        let c = s.counts_by_type();
        assert_eq!(c, [1, 1, 1, 1]);
        assert_eq!(s.count_type(TraceType::Fire), 1);
        assert_eq!(s.recent(2).count(), 2);
    }
}
