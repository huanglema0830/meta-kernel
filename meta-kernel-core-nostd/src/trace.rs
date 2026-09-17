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

//! 【2.3b 片4 迁移】与 `meta-kernel-core/src/trace.rs` **同源**；仅作下述适配，其余**逐行逐字未改**：
//! ① 补 `alloc`/`core` 的 `use`（no_std 下 `Vec`/`vec!`/`String`/`ToString`/`format!` 不在 prelude）
//! ② `std::cmp::Ordering` → `core::cmp::Ordering`（同一类型）
//! ③ 引入 `FloatOps` trait ⇒ 浮点方法在 no_std 下解析到 `fmath`（调用点一行未改）
//! 说明：本片由「用户指定的 5 模块」**扩为 9 模块** —— 原 5 个**反向依赖** `trace`/`dna_generate`/`dna_trace`/`gene_library`，**不封闭就编不过**（见报告 §三）
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
#[allow(unused_imports)] // host(std) 下内在方法优先 ⇒ 本 import 可能"未使用"，这是 FloatOps 机制的必然结果
use crate::fmath::FloatOps;

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
    /// 指纹（同模式 → 同指纹 → 习气累积；基于**能量流模式**+数值模式）。
    pub fingerprint: u64,
    /// 产生时的能量流状态（absorbed 入流口径，来自内核能量池）。
    pub energy_flow: f32,
}

/// 痕迹指纹：基于**能量流模式**（能量桶优先）叠合数值模式
/// （均值桶×32 + 标准差桶×16 + 长度盐）。
pub fn fingerprint_of(samples: &[f32], energy_flow: f32) -> u64 {
    let n = samples.len();
    if n == 0 {
        return 0;
    }
    let mean = samples.iter().sum::<f32>() / n as f32;
    let var = samples.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n as f32;
    let std = var.sqrt();
    let mb = ((mean * 31.999).round() as u64).min(31);
    let sb = ((std * 15.999).round() as u64).min(15);
    // 能量流模式：0..1 量化为 4bit（16 级）
    let fb = ((energy_flow.clamp(0.0, 1.0) * 15.999).round() as u64).min(15);
    (fb << 12) | (mb << 8) | (sb << 4) | (n as u64 & 0xF)
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
#[derive(Debug, Clone)]
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
    /// 容量上限（序列化往返时保持不变）。
    pub fn cap(&self) -> usize {
        self.cap
    }
    /// 全部痕迹（时间正序），供落盘前检查与往返比对。
    pub fn all(&self) -> &[Trace] {
        &self.traces
    }
    /// 序列化：每行一条痕迹（**无表头**；空存储 → 空串）。
    ///
    /// 零依赖纯文本编解码——**序列化在内核，文件 IO 在宿主**（内核无 IO 红线）。
    pub fn to_text(&self) -> String {
        let mut s = String::with_capacity(self.traces.len() * 40);
        for t in &self.traces {
            s.push_str(&t.to_text());
            s.push('\n');
        }
        s
    }
    /// 反序列化：忽略空行与非法行；超出 `cap` 只保留**最近** `cap` 条（与 `record` 同口径）。
    pub fn from_text(text: &str, cap: usize) -> Self {
        let mut v: Vec<Trace> = Vec::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            if let Some(t) = Trace::from_text(l) {
                v.push(t);
            }
        }
        if v.len() > cap {
            let drop_n = v.len() - cap;
            v.drain(..drop_n);
        }
        Self { traces: v, cap }
    }
}

impl Trace {
    /// 单行文本：`step|intensity|typecode|fingerprint|energy_flow`。
    pub fn to_text(&self) -> String {
        format!(
            "{}|{:.6}|{}|{}|{:.6}",
            self.step,
            self.intensity,
            self.trace_type.code(),
            self.fingerprint,
            self.energy_flow
        )
    }
    /// 反序列化（字段数不符 / 数值非法 / 类型码越界 → `None`）。
    pub fn from_text(line: &str) -> Option<Self> {
        let p: Vec<&str> = line.split('|').collect();
        if p.len() != 5 {
            return None;
        }
        let step: u64 = p[0].trim().parse().ok()?;
        let intensity: f32 = p[1].trim().parse().ok()?;
        let code: u32 = p[2].trim().parse().ok()?;
        let fingerprint: u64 = p[3].trim().parse().ok()?;
        let energy_flow: f32 = p[4].trim().parse().ok()?;
        if !intensity.is_finite() || !energy_flow.is_finite() {
            return None;
        }
        let trace_type = match code {
            0 => TraceType::Wind,
            1 => TraceType::Fire,
            2 => TraceType::Water,
            3 => TraceType::Earth,
            _ => return None,
        };
        Some(Trace { step, intensity, trace_type, fingerprint, energy_flow })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_repeats_for_same_pattern() {
        let a: Vec<f32> = (0..16).map(|i| (i % 3) as f32 / 3.0).collect();
        let b: Vec<f32> = (0..16).map(|i| (i % 3) as f32 / 3.0).collect();
        assert_eq!(fingerprint_of(&a, 0.4), fingerprint_of(&b, 0.4));
        assert_eq!(fingerprint_of(&a, 0.4), fingerprint_of(&a, 0.4));
        // 能量流不同 → 指纹不同（验收：指纹基于能量流模式）
        assert_ne!(fingerprint_of(&a, 0.2), fingerprint_of(&a, 0.8));
    }

    #[test]
    fn trace_carries_energy_flow() {
        // 验收：痕迹包含能量流信息
        let mut s = TraceStore::new();
        s.record(Trace { step: 1, intensity: 0.6, trace_type: TraceType::Fire, fingerprint: 9, energy_flow: 0.72 });
        let t = s.recent(1).next().unwrap();
        assert!((t.energy_flow - 0.72).abs() < 1e-6, "能量流应被记录: {}", t.energy_flow);
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
        s.record(Trace { step: 1, intensity: 0.5, trace_type: TraceType::Wind, fingerprint: 7, energy_flow: 0.5 });
        s.record(Trace { step: 2, intensity: 0.6, trace_type: TraceType::Fire, fingerprint: 7, energy_flow: 0.6 });
        s.record(Trace { step: 3, intensity: 0.4, trace_type: TraceType::Water, fingerprint: 8, energy_flow: 0.4 });
        s.record(Trace { step: 4, intensity: 0.3, trace_type: TraceType::Earth, fingerprint: 8, energy_flow: 0.3 });
        assert_eq!(s.len(), 4);
        let c = s.counts_by_type();
        assert_eq!(c, [1, 1, 1, 1]);
        assert_eq!(s.count_type(TraceType::Fire), 1);
        assert_eq!(s.recent(2).count(), 2);
    }

    fn sample_trace(step: u64, tt: TraceType, fp: u64) -> Trace {
        Trace { step, intensity: 0.42, trace_type: tt, fingerprint: fp, energy_flow: 0.55 }
    }

    /// 文本编解码保留 6 位小数 → 浮点字段按 **1e-6 容差**比对（整数字段逐位比）。
    /// 度量铁律：比对打在**真实语义**（每条痕迹的字段）上，不比"非空"了事。
    fn assert_trace_eq(a: &Trace, b: &Trace) {
        assert_eq!(a.step, b.step, "step 逐位一致");
        assert_eq!(a.trace_type, b.trace_type, "类型逐位一致");
        assert_eq!(a.fingerprint, b.fingerprint, "指纹逐位一致");
        assert!((a.intensity - b.intensity).abs() < 1e-6, "intensity {} vs {}", a.intensity, b.intensity);
        assert!((a.energy_flow - b.energy_flow).abs() < 1e-6, "energy_flow {} vs {}", a.energy_flow, b.energy_flow);
    }

    fn assert_store_eq(a: &TraceStore, b: &TraceStore) {
        assert_eq!(a.len(), b.len(), "条数一致");
        for (x, y) in a.all().iter().zip(b.all().iter()) {
            assert_trace_eq(x, y);
        }
    }

    #[test]
    fn trace_text_roundtrip_preserves_fields() {
        let t = sample_trace(7, TraceType::Fire, 12345);
        let back = Trace::from_text(&t.to_text()).expect("自编码必须可解码");
        assert_trace_eq(&back, &t);
    }

    #[test]
    fn trace_from_text_rejects_malformed() {
        assert!(Trace::from_text("").is_none());
        assert!(Trace::from_text("1|0.5|0").is_none(), "字段数不符");
        assert!(Trace::from_text("x|0.5|0|1|0.5").is_none(), "step 非法");
        assert!(Trace::from_text("1|0.5|9|1|0.5").is_none(), "类型码越界");
        assert!(Trace::from_text("1|NaN|0|1|0.5").is_none(), "NaN 拒绝");
    }

    #[test]
    fn store_text_roundtrip_preserves_all_traces() {
        let mut s = TraceStore::new();
        for i in 1..=5u64 {
            s.record(sample_trace(i, TraceType::Wind, 100 + i));
        }
        let back = TraceStore::from_text(&s.to_text(), s.cap());
        assert_store_eq(&back, &s);
        assert_eq!(back.cap(), s.cap(), "容量往返保持");
        assert_eq!(back.counts_by_type(), s.counts_by_type(), "类型分布一致");
    }

    #[test]
    fn store_from_text_ignores_bad_lines_and_empty() {
        let mut s = TraceStore::new();
        s.record(sample_trace(1, TraceType::Earth, 9));
        let dirty = format!("garbage line\n\n{}\n1|0.5|0\n", s.to_text());
        let back = TraceStore::from_text(&dirty, s.cap());
        assert_eq!(back.len(), 1, "非法行被忽略，只留 1 条");
        assert_eq!(back.all()[0].fingerprint, 9);
        assert!(TraceStore::from_text("", 16).is_empty(), "空文本 → 空存储");
    }

    #[test]
    fn store_from_text_truncates_to_cap_keeping_latest() {
        let mut s = TraceStore::new();
        for i in 1..=10u64 {
            s.record(sample_trace(i, TraceType::Wind, i));
        }
        let back = TraceStore::from_text(&s.to_text(), 3);
        assert_eq!(back.len(), 3, "超容量截断到 cap");
        let fps: Vec<u64> = back.all().iter().map(|t| t.fingerprint).collect();
        assert_eq!(fps, vec![8, 9, 10], "保留最近 3 条（与 record 同口径）");
    }
}
