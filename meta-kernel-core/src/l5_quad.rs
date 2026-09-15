//! L5 · **四元组内在变量**（紧张/平静/喜欢/安全）——框架落地。
//!
//! 依据：发起人「四元组内在变量框架落地」（v0.112）。正式定义：
//!
//! | 变量 | 对应 | 含义 |
//! |---|---|---|
//! | **紧张 tension** | 多巴胺 | 需求未满足的累积张力 |
//! | **平静 calm** | 血清素 | 无威胁时的稳定基线 |
//! | **喜欢 liking** | 内啡肽 | 需求满足后的释放 |
//! | **安全 safety** | 催产素 | 社会信任与连接 |
//!
//! ## 诊断逻辑（与发起人描述逐条对应）
//! ```text
//! 被动接收扰动 P → 更新四元组 → 对比本底场（四元组基线）→ 输出诊断 + 四元组状态
//! ```
//! ## 基因库存什么（"基因库升级"）
//! 两条映射都**存在基因库**，改库即改行为：
//! - **P → ΔQ**：关系层（公式层）加权公式 `quadmap.p2q.{tension,calm,liking,safety}`；
//! - **ΔQ → B**：行为倾向由**阈值常量**判定，常量存基础公式层 `quad.tension_high` 等。
//!
//! ## 探测策略
//! **默认被动（水面模式）**；**主动仅为例外**——只有当感知困难（置信度低）时才主动探测，
//! 且必须给出理由（[`ProbeReason`]），不允许"无理由地主动打扰"。

use crate::gene_library::{Formula, GeneLibrary};

/// e^-0.1 —— 自然回归系数（无扰动时向基线回，与能量回归同口径）。
pub const NATURAL_RETURN: f64 = 0.904_837_418_035_959_5;

/// 四元组状态（各 0..1）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub tension: f64,
    pub calm: f64,
    pub liking: f64,
    pub safety: f64,
}

impl Default for Quad {
    /// 默认基线：平静略高、紧张低（"无事发生时"的稳态）。
    fn default() -> Self {
        Self { tension: 0.2, calm: 0.6, liking: 0.5, safety: 0.6 }
    }
}

impl Quad {
    pub fn to_array(&self) -> [f64; 4] {
        [self.tension, self.calm, self.liking, self.safety]
    }
    pub fn from_array(a: [f64; 4]) -> Self {
        Self { tension: a[0].clamp(0.0, 1.0), calm: a[1].clamp(0.0, 1.0), liking: a[2].clamp(0.0, 1.0), safety: a[3].clamp(0.0, 1.0) }
    }
    /// 序列化为文本：`tension,calm,liking,safety`（6 位小数）。
    ///
    /// 零依赖纯文本编解码——**序列化在内核，文件 IO 在宿主**（内核无 IO 红线）。
    pub fn to_text(&self) -> String {
        let a = self.to_array();
        format!("{:.6},{:.6},{:.6},{:.6}", a[0], a[1], a[2], a[3])
    }
    /// 反序列化（字段数不符 / 数值非法 → `None`）；值钳制到 0..1（与 `from_array` 同口径）。
    pub fn from_text(s: &str) -> Option<Self> {
        let p: Vec<&str> = s.split(',').collect();
        if p.len() != 4 {
            return None;
        }
        let mut a = [0f64; 4];
        for (i, v) in p.iter().enumerate() {
            let x: f64 = v.trim().parse().ok()?;
            if !x.is_finite() {
                return None;
            }
            a[i] = x;
        }
        Some(Self::from_array(a))
    }
    /// 主导变量（值最高者）。
    pub fn dominant(&self) -> (usize, f64) {
        let a = self.to_array();
        let mut bi = 0;
        for i in 1..4 {
            if a[i] > a[bi] {
                bi = i;
            }
        }
        (bi, a[bi])
    }
    pub fn dominant_label(&self) -> &'static str {
        ["紧张", "平静", "喜欢", "安全"][self.dominant().0]
    }
}

/// 扰动（被动接收）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Perturbation {
    /// 四场强度（地/水/火/风），0..1。
    pub p: [f64; 4],
    /// 满足度 0..1（本轮扰动**被满足**的程度 → 推动"喜欢"）。
    pub satisfaction: f64,
    /// 威胁感 0..1（本轮扰动带**威胁**的程度 → 压低"安全"）。
    pub threat: f64,
}

impl Perturbation {
    pub fn new(p: [f64; 4], satisfaction: f64, threat: f64) -> Self {
        Self { p, satisfaction: satisfaction.clamp(0.0, 1.0), threat: threat.clamp(0.0, 1.0) }
    }
    /// 由四场读数构造（与 L1 解析器直接对接）。
    pub fn from_fields(r: &crate::l1_field_parse::FieldReading, satisfaction: f64, threat: f64) -> Self {
        Self::new(r.to_array(), satisfaction, threat)
    }
}

/// 行为倾向 B。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tendency {
    /// 稳住（继续当前节奏）。
    Hold,
    /// 推进（趁势做成）。
    Push,
    /// 求助（连接他人）。
    SeekHelp,
    /// 撤退（先保护自己）。
    Withdraw,
    /// 等待（不动，观察）。
    Wait,
}

impl Tendency {
    pub fn label(&self) -> &'static str {
        match self {
            Tendency::Hold => "稳住",
            Tendency::Push => "推进",
            Tendency::SeekHelp => "求助",
            Tendency::Withdraw => "撤退",
            Tendency::Wait => "等待",
        }
    }
    pub fn code(&self) -> u8 {
        match self {
            Tendency::Hold => 1,
            Tendency::Push => 2,
            Tendency::SeekHelp => 3,
            Tendency::Withdraw => 4,
            Tendency::Wait => 5,
        }
    }
}

/// ΔQ→B 的阈值（**存基因库基础公式层**；改库即改倾向判定）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadRules {
    pub tension_high: f64,
    pub calm_low: f64,
    pub liking_high: f64,
    pub safety_low: f64,
}

impl Default for QuadRules {
    fn default() -> Self {
        Self { tension_high: 0.7, calm_low: 0.35, liking_high: 0.65, safety_low: 0.35 }
    }
}

pub const RULE_NAMES: [&str; 4] = ["quad.tension_high", "quad.calm_low", "quad.liking_high", "quad.safety_low"];

/// P→ΔQ 的映射公式名（存在**关系层/公式层**）。
pub const MAP_NAMES: [&str; 4] = ["quadmap.p2q.tension", "quadmap.p2q.calm", "quadmap.p2q.liking", "quadmap.p2q.safety"];

/// 内置默认映射（四场顺序 = 地/水/火/风）。
///
/// 取值依据（可解释）：
/// - **紧张 ← 火**（刺激/强度最直接推紧张）｜**平静 ← 火取负**（火高则平静被压低）；
/// - **喜欢 ← 水**（信息连续体最容易被"满足"）｜**安全 ← 地**（结构稳 = 可依靠）。
pub const DEFAULT_P2Q: [[f64; 4]; 4] = [
    [0.15, 0.15, 0.60, 0.30],  // Δtension  = 0.15·地 + 0.15·水 + 0.60·火 + 0.30·风
    [-0.10, -0.10, -0.45, -0.25], // Δcalm     = 负向（扰动压低平静）
    [0.10, 0.55, 0.20, 0.10],  // Δliking  = 主要来自水（满足）
    [0.45, 0.15, -0.20, -0.10], // Δsafety  = 地（结构）为主，火风略负
];

fn enc(w: &[f64; 4]) -> [f64; 7] {
    [w[0], w[1], w[2], w[3], 0.0, 0.0, 0.0]
}

/// **把映射与规则登记进基因库**（幂等；返回登记条数）。
pub fn seed_into(lib: &mut GeneLibrary) -> usize {
    for i in 0..4 {
        lib.set_relation_formula(MAP_NAMES[i], Formula::Weighted { dims: 4, w: enc(&DEFAULT_P2Q[i]) }, [1.0; 7]);
    }
    let r = QuadRules::default();
    lib.set_base_constant(RULE_NAMES[0], r.tension_high, [1.0; 7]);
    lib.set_base_constant(RULE_NAMES[1], r.calm_low, [1.0; 7]);
    lib.set_base_constant(RULE_NAMES[2], r.liking_high, [1.0; 7]);
    lib.set_base_constant(RULE_NAMES[3], r.safety_low, [1.0; 7]);
    8
}

/// 读取 P→ΔQ 权重（基因库优先，缺项回退默认）。
pub fn weights_of(lib: &GeneLibrary, i: usize) -> [f64; 4] {
    if i >= 4 {
        return [0.0; 4];
    }
    if let Some(Formula::Weighted { w, .. }) = lib.relation_formula(MAP_NAMES[i]) {
        return [w[0], w[1], w[2], w[3]];
    }
    DEFAULT_P2Q[i]
}

/// 读取 ΔQ→B 规则（基因库优先，缺项回退默认）。
pub fn rules_of(lib: &GeneLibrary) -> QuadRules {
    let d = QuadRules::default();
    QuadRules {
        tension_high: lib.base_constant(RULE_NAMES[0]).unwrap_or(d.tension_high),
        calm_low: lib.base_constant(RULE_NAMES[1]).unwrap_or(d.calm_low),
        liking_high: lib.base_constant(RULE_NAMES[2]).unwrap_or(d.liking_high),
        safety_low: lib.base_constant(RULE_NAMES[3]).unwrap_or(d.safety_low),
    }
}

/// **P → ΔQ**（有符号增量；负值有意义，**不夹到 0**）。
pub fn delta_signed(p: &Perturbation, lib: &GeneLibrary) -> [f64; 4] {
    let mut out = [0.0f64; 4];
    for i in 0..4 {
        let w = weights_of(lib, i);
        let mut s = 0.0;
        for k in 0..4 {
            s += w[k] * p.p[k];
        }
        out[i] = s;
    }
    // 满足度推动"喜欢"、压低"紧张"；威胁感压低"安全"（**语义显式**，不藏在权重里）
    out[2] += 0.5 * p.satisfaction;
    out[0] -= 0.3 * p.satisfaction;
    out[3] -= 0.5 * p.threat;
    out
}

/// **P → ΔQ**（对外读数版：负增量按 0 呈现，便于直接观察"涨了多少"）。
pub fn delta_of(p: &Perturbation, lib: &GeneLibrary) -> Quad {
    Quad::from_array(delta_signed(p, lib))
}

/// **被动更新**：`quad' = clamp(quad + ΔQ)`。
pub fn update(quad: Quad, p: &Perturbation, lib: &GeneLibrary) -> Quad {
    let d = delta_signed(p, lib);
    let a = quad.to_array();
    Quad::from_array([a[0] + d[0], a[1] + d[1], a[2] + d[2], a[3] + d[3]])
}

/// **自然回归**：无扰动时向基线回落（每步 ×e^-0.1）。
pub fn regress(quad: Quad, baseline: Quad) -> Quad {
    let a = quad.to_array();
    let b = baseline.to_array();
    let mut o = [0.0f64; 4];
    for i in 0..4 {
        o[i] = b[i] + (a[i] - b[i]) * NATURAL_RETURN;
    }
    Quad::from_array(o)
}

/// **ΔQ → B**：行为倾向（阈值取自基因库）。
pub fn tendency_of(quad: Quad, lib: &GeneLibrary) -> Tendency {
    let r = rules_of(lib);
    if quad.safety < r.safety_low {
        // 安全感不足：先保护或先连接，视紧张而定
        return if quad.tension > r.tension_high { Tendency::Withdraw } else { Tendency::SeekHelp };
    }
    if quad.tension > r.tension_high && quad.calm < r.calm_low {
        return Tendency::Wait; // 又紧又不稳 → 不动
    }
    if quad.liking > r.liking_high && quad.tension < r.tension_high {
        return Tendency::Push; // 顺畅且有释放 → 推进
    }
    if quad.tension > r.tension_high {
        return Tendency::Wait;
    }
    Tendency::Hold
}

/// 四元组诊断（输出给 L6/L7）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadDiagnosis {
    pub quad: Quad,
    /// 与本底（基线）的偏离强度（0..1）。
    pub deviation: f64,
    pub dominant: &'static str,
    pub tendency: Tendency,
    pub confidence: f64,
}

/// **诊断**：对比本底场 → 输出诊断 + 四元组状态。
pub fn diagnose(quad: Quad, baseline: Quad, lib: &GeneLibrary, confidence: f64) -> QuadDiagnosis {
    let a = quad.to_array();
    let b = baseline.to_array();
    let mut acc = 0.0;
    for i in 0..4 {
        acc += (a[i] - b[i]).abs();
    }
    QuadDiagnosis {
        quad,
        deviation: (acc / 4.0).clamp(0.0, 1.0),
        dominant: quad.dominant_label(),
        tendency: tendency_of(quad, lib),
        confidence: confidence.clamp(0.0, 1.0),
    }
}

// ===== 探测策略：默认被动（水面模式），主动仅作例外 =====

/// 探测模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeMode {
    /// **默认**：水面模式（只被动接收，不主动探测）。
    Passive,
    /// 例外：感知困难时的主动探测。
    Active,
}

/// 主动探测的理由（**不允许无理由主动**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeReason {
    /// 置信度过低（看不清）。
    LowConfidence,
    /// 四元组偏离过大（异常）。
    HighDeviation,
    /// 长时间无信号。
    StaleSignal,
}

/// 探测决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeDecision {
    pub mode: ProbeMode,
    pub reason: Option<ProbeReason>,
}

/// 探测策略参数（阈值存常量，便于统一调整）。
pub const PROBE_CONFIDENCE_FLOOR: f64 = 0.35;
pub const PROBE_DEVIATION_CEIL: f64 = 0.45;

/// **决定是否主动探测（可指定门槛）**：默认被动；仅当"感知困难"才主动，且必须带理由。
///
/// `floor` 为置信度门槛——由 `l5_attention` 按**注意力**（四元组）调节：
/// 紧张高 → 门槛降低 → 更易转主动；平静高 → 门槛升高 → 更被动。
/// **注意**：门槛只改变"多容易转主动"，**不取消**"必须带理由"这条纪律。
pub fn decide_probe_with_floor(
    confidence: f64,
    deviation: f64,
    stale: bool,
    floor: f64,
) -> ProbeDecision {
    if stale {
        return ProbeDecision { mode: ProbeMode::Active, reason: Some(ProbeReason::StaleSignal) };
    }
    let floor = if floor.is_finite() { floor } else { PROBE_CONFIDENCE_FLOOR };
    if confidence < floor {
        return ProbeDecision { mode: ProbeMode::Active, reason: Some(ProbeReason::LowConfidence) };
    }
    if deviation > PROBE_DEVIATION_CEIL {
        return ProbeDecision { mode: ProbeMode::Active, reason: Some(ProbeReason::HighDeviation) };
    }
    ProbeDecision { mode: ProbeMode::Passive, reason: None }
}

/// **决定是否主动探测**（用默认门槛 `PROBE_CONFIDENCE_FLOOR`）。
pub fn decide_probe(confidence: f64, deviation: f64, stale: bool) -> ProbeDecision {
    decide_probe_with_floor(confidence, deviation, stale, PROBE_CONFIDENCE_FLOOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> GeneLibrary {
        let mut l = GeneLibrary::new();
        seed_into(&mut l);
        l
    }

    #[test]
    fn defaults_are_stable_and_labelled() {
        let q = Quad::default();
        assert_eq!(q.dominant_label(), "平静");
        assert!((0.0..=1.0).contains(&q.tension) && q.to_array().len() == 4);
        for t in [Tendency::Hold, Tendency::Push, Tendency::SeekHelp, Tendency::Withdraw, Tendency::Wait] {
            assert!(!t.label().is_empty() && t.code() >= 1);
        }
    }

    #[test]
    fn seed_is_idempotent_and_readable_from_library() {
        let mut l = GeneLibrary::new();
        assert_eq!(seed_into(&mut l), 8);
        seed_into(&mut l);
        assert_eq!(l.relation.len(), 4, "4 条 P→ΔQ 映射入关系层");
        assert_eq!(l.base.len(), 4, "4 条 ΔQ→B 阈值入基础层");
        for i in 0..4 {
            assert_eq!(weights_of(&l, i), DEFAULT_P2Q[i]);
        }
        assert_eq!(rules_of(&l), QuadRules::default());
    }

    #[test]
    fn missing_genes_fall_back() {
        let empty = GeneLibrary::new();
        for i in 0..4 {
            assert_eq!(weights_of(&empty, i), DEFAULT_P2Q[i]);
        }
        assert_eq!(rules_of(&empty), QuadRules::default());
        assert_eq!(weights_of(&empty, 9), [0.0; 4]);
    }

    /// **验收：扰动 → 更新四元组**（火强 → 紧张上升、平静下降）。
    #[test]
    fn perturbation_updates_quad_as_designed() {
        let l = lib();
        let q0 = Quad::default();
        let hot = Perturbation::new([0.2, 0.2, 1.0, 0.1], 0.0, 0.0);
        let q1 = update(q0, &hot, &l);
        assert!(q1.tension > q0.tension, "紧张上升: {} -> {}", q0.tension, q1.tension);
        assert!(q1.calm < q0.calm, "平静下降: {} -> {}", q0.calm, q1.calm);
        // 满足 → 喜欢上升；威胁 → 安全下降
        let q2 = update(q0, &Perturbation::new([0.5, 0.8, 0.1, 0.1], 1.0, 0.0), &l);
        assert!(q2.liking > q0.liking, "满足推高喜欢");
        let q3 = update(q0, &Perturbation::new([0.5, 0.5, 0.5, 0.5], 0.0, 1.0), &l);
        assert!(q3.safety < q0.safety, "威胁压低安全");
    }

    #[test]
    fn quad_is_always_bounded_even_under_extreme_perturbation() {
        let l = lib();
        let q = update(Quad::default(), &Perturbation::new([1.0; 4], 1.0, 1.0), &l);
        for v in q.to_array() {
            assert!((0.0..=1.0).contains(&v), "越界 {v}");
        }
        let q2 = update(Quad::from_array([0.0; 4]), &Perturbation::new([0.0; 4], 0.0, 0.0), &l);
        for v in q2.to_array() {
            assert!((0.0..=1.0).contains(&v));
        }
    }

    /// **验收：无扰动时回归基线**（×e^-0.1）。
    #[test]
    fn regress_returns_toward_baseline() {
        let base = Quad::default();
        let excited = Quad::from_array([1.0, 0.0, 1.0, 0.0]);
        let r1 = regress(excited, base);
        assert!((r1.tension - base.tension).abs() < (excited.tension - base.tension).abs(), "向基线靠近");
        let mut q = excited;
        for _ in 0..200 {
            q = regress(q, base);
        }
        for i in 0..4 {
            assert!((q.to_array()[i] - base.to_array()[i]).abs() < 1e-6, "长期回归基线");
        }
    }

    /// **验收：ΔQ → B**（阈值可改 → 倾向改变）。
    #[test]
    fn tendency_follows_rules_and_library_override() {
        let mut l = lib();
        // 安全低 + 紧张高 → 撤退
        let danger = Quad::from_array([0.9, 0.5, 0.5, 0.1]);
        assert_eq!(tendency_of(danger, &l), Tendency::Withdraw);
        // 安全低但不紧张 → 求助
        assert_eq!(tendency_of(Quad::from_array([0.3, 0.5, 0.5, 0.1]), &l), Tendency::SeekHelp);
        // 顺畅有释放 → 推进
        assert_eq!(tendency_of(Quad::from_array([0.3, 0.6, 0.8, 0.7]), &l), Tendency::Push);
        // 又紧又不稳 → 等待 / 平常 → 稳住
        assert_eq!(tendency_of(Quad::from_array([0.9, 0.2, 0.4, 0.7]), &l), Tendency::Wait);
        assert_eq!(tendency_of(Quad::from_array([0.3, 0.6, 0.4, 0.7]), &l), Tendency::Hold);
        // **改基因库 → 倾向判定改变**
        l.set_base_constant(RULE_NAMES[3], 0.95, [1.0; 7]); // 把"安全低"阈值抬到 0.95
        assert_eq!(tendency_of(Quad::from_array([0.3, 0.6, 0.8, 0.7]), &l), Tendency::SeekHelp,
            "改阈值后原判定改变");
    }

    /// **验收：P→ΔQ 映射可学习**（改基因库即改更新结果）。
    #[test]
    fn learned_p2q_changes_update() {
        let mut l = lib();
        let p = Perturbation::new([0.0, 0.0, 1.0, 0.0], 0.0, 0.0);
        let before = update(Quad::default(), &p, &l).tension;
        // 把"紧张"的来源从火改到地
        l.set_relation_formula(MAP_NAMES[0], Formula::Weighted { dims: 4, w: enc(&[0.9, 0.0, 0.0, 0.0]) }, [1.0; 7]);
        let after = update(Quad::default(), &p, &l).tension;
        assert_ne!(before, after, "改映射 → 更新结果改变");
        assert_eq!(weights_of(&l, 0), [0.9, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn diagnosis_compares_with_baseline() {
        let l = lib();
        let base = Quad::default();
        let same = diagnose(base, base, &l, 0.9);
        assert!(same.deviation < 1e-9, "与基线相同 → 偏离 0");
        let far = diagnose(Quad::from_array([1.0, 0.0, 1.0, 0.0]), base, &l, 0.4);
        assert!(far.deviation > 0.2, "偏离显著: {}", far.deviation);
        assert_eq!(far.dominant, "紧张");
        assert!((far.confidence - 0.4).abs() < 1e-9);
    }

    /// **验收：探测默认被动，主动仅例外且必带理由**。
    #[test]
    fn probing_is_passive_by_default_and_active_only_when_difficult() {
        let d0 = decide_probe(0.9, 0.1, false);
        assert_eq!(d0.mode, ProbeMode::Passive);
        assert!(d0.reason.is_none(), "被动不带理由");

        let d1 = decide_probe(0.2, 0.1, false);
        assert_eq!(d1.mode, ProbeMode::Active);
        assert_eq!(d1.reason, Some(ProbeReason::LowConfidence));

        let d2 = decide_probe(0.9, 0.9, false);
        assert_eq!(d2.reason, Some(ProbeReason::HighDeviation));

        let d3 = decide_probe(0.9, 0.1, true);
        assert_eq!(d3.reason, Some(ProbeReason::StaleSignal));

        // 阈值边界
        assert_eq!(decide_probe(PROBE_CONFIDENCE_FLOOR, 0.0, false).mode, ProbeMode::Passive);
        assert_eq!(decide_probe(PROBE_CONFIDENCE_FLOOR - 0.01, 0.0, false).mode, ProbeMode::Active);
    }

    #[test]
    fn perturbation_from_fields_bridges_l1() {
        use crate::l1_field_parse::{parse, PageSignal};
        let r = parse(&PageSignal { text_len: 4000, paragraph_count: 15, heading_count: 4, ..Default::default() });
        let p = Perturbation::from_fields(&r, 0.5, 0.0);
        assert_eq!(p.p, r.to_array());
        assert!((p.satisfaction - 0.5).abs() < 1e-9);
        let q = update(Quad::default(), &p, &lib());
        for v in q.to_array() {
            assert!((0.0..=1.0).contains(&v));
        }
    }

    #[test]
    fn quad_text_roundtrip_preserves_four_values() {
        let q = Quad { tension: 0.31, calm: 0.62, liking: 0.48, safety: 0.75 };
        let text = q.to_text();
        let back = Quad::from_text(&text).expect("自编码必须可解码");
        for i in 0..4 {
            assert!(
                (back.to_array()[i] - q.to_array()[i]).abs() < 1e-6,
                "第 {i} 维 {} vs {}",
                back.to_array()[i],
                q.to_array()[i]
            );
        }
        assert_eq!(back.dominant_label(), q.dominant_label(), "主导变量也一致");
    }

    #[test]
    fn quad_from_text_rejects_malformed_and_clamps() {
        assert!(Quad::from_text("").is_none());
        assert!(Quad::from_text("0.1,0.2,0.3").is_none(), "维度数不符");
        assert!(Quad::from_text("a,0.2,0.3,0.4").is_none(), "数值非法");
        assert!(Quad::from_text("0.1,0.2,0.3,NaN").is_none(), "NaN 拒绝");
        // 越界值钳制到 0..1（与 from_array 同口径）
        let c = Quad::from_text("1.7,-0.2,0.5,0.6").expect("可解码");
        assert_eq!(c.to_array(), [1.0, 0.0, 0.5, 0.6], "越界钳制");
    }

    #[test]
    fn quad_text_survives_baseline_and_excited_states() {
        // 持久化的两个真实端点：默认基线 与 全激状态
        for q in [Quad::default(), Quad::from_array([1.0, 0.0, 1.0, 0.0])] {
            let back = Quad::from_text(&q.to_text()).expect("可解码");
            for i in 0..4 {
                assert!((back.to_array()[i] - q.to_array()[i]).abs() < 1e-6);
            }
        }
    }
}
