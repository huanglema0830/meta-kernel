//! L5 · 诊断证据（l5_evidence）——**世界模型匹配度** 与 **预测误差** → 置信度依据。
//!
//! 依据发起人 v0.123：
//! ① **世界模型接入 L5 诊断层**：诊断不只看当前场域状态，还看**世界模型中的历史模式**；
//!    匹配 → 置信度提高；不匹配 → 置信度降低。
//! ② **预测误差接入 L5 诊断层**：误差低 → 置信度高；误差高 → 置信度低；
//!    且诊断结论必须**携带**该依据（谁把置信度抬高/压低，一望可知）。
//!
//! ## 纪律（不破戒律）
//! - **零依赖**（内核红线）：只用 f64 / Vec / String。
//! - **不改语义**：本模块只做**置信度的证据加权**，绝不改变"单一主结论"（不邪淫）、
//!   也不改变结论的因果链（一因一果）；结论本身仍由场域自身推导（不饮酒）。
//! - **权重进基因库**：`diag.world_gain` / `diag.pe_gain` / `diag.pe_ref` / `diag.maturity_ref`
//!   —— **改库即改判据**。
//! - **证据不足即不改变**：世界模型不成熟（观测太少）时，匹配度向 0.5 收缩 → 修正量趋 0。
//!   这是"不妄语"在本层的落点：**没有把握就不动结论**。

//! 【2.3b 片4 迁移】与 `meta-kernel-core/src/l5_evidence.rs` **同源**；仅作下述适配，其余**逐行逐字未改**：
//! ① 补 `alloc`/`core` 的 `use`（no_std 下 `Vec`/`vec!`/`String`/`ToString`/`format!` 不在 prelude）
//! ② `std::cmp::Ordering` → `core::cmp::Ordering`（同一类型）
//! ③ 引入 `FloatOps` trait ⇒ 浮点方法在 no_std 下解析到 `fmath`（调用点一行未改）
//! 说明：本片由「用户指定的 5 模块」**扩为 9 模块** —— 原 5 个**反向依赖** `trace`/`dna_generate`/`dna_trace`/`gene_library`，**不封闭就编不过**（见报告 §三）
use alloc::string::String;
use alloc::format;
#[allow(unused_imports)] // host(std) 下内在方法优先 ⇒ 本 import 可能"未使用"，这是 FloatOps 机制的必然结果
use crate::fmath::FloatOps;

use crate::gene_library::GeneLibrary;
use crate::l1_field_parse::FieldReading;
use crate::l3_world::WorldModel;

// ===== 系数（默认值；可由基因库覆盖）=====

/// 世界模型匹配度对置信度的最大修正幅度。
pub const WORLD_GAIN_DEFAULT: f64 = 0.15;
/// 预测误差对置信度的最大修正幅度。
pub const PE_GAIN_DEFAULT: f64 = 0.20;
/// 预测误差参考值（实测口径下"正常水平"；误差等于它时修正为 0）。
pub const PE_REF_DEFAULT: f64 = 0.05;
/// 世界模型成熟所需的观测次数（达到即成熟度 1.0）。
pub const MATURITY_REF_DEFAULT: f64 = 8.0;
/// 局部匹配（当前页面/站点自身条目）在综合匹配中的权重。
pub const LOCAL_WEIGHT: f64 = 0.65;

/// 基因库中的系数名（基础公式层常量）。
pub const GAIN_NAMES: [&str; 4] =
    ["diag.world_gain", "diag.pe_gain", "diag.pe_ref", "diag.maturity_ref"];

/// 系数默认值（与 `GAIN_NAMES` 一一对应）。
pub const GAIN_DEFAULTS: [f64; 4] =
    [WORLD_GAIN_DEFAULT, PE_GAIN_DEFAULT, PE_REF_DEFAULT, MATURITY_REF_DEFAULT];

/// 证据权重（基因库优先，缺项回退默认）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gains {
    pub world_gain: f64,
    pub pe_gain: f64,
    pub pe_ref: f64,
    pub maturity_ref: f64,
}

impl Default for Gains {
    fn default() -> Self {
        Self {
            world_gain: WORLD_GAIN_DEFAULT,
            pe_gain: PE_GAIN_DEFAULT,
            pe_ref: PE_REF_DEFAULT,
            maturity_ref: MATURITY_REF_DEFAULT,
        }
    }
}

/// 把默认系数写入基因库（幂等 upsert）。
pub fn seed_into(lib: &mut GeneLibrary) -> usize {
    for i in 0..GAIN_NAMES.len() {
        lib.set_base_constant(GAIN_NAMES[i], GAIN_DEFAULTS[i], [1.0; 7]);
    }
    GAIN_NAMES.len()
}

/// 从基因库读系数（缺项回退默认）。
pub fn gains_of(lib: &GeneLibrary) -> Gains {
    let mut g = Gains::default();
    if let Some(v) = lib.base_constant(GAIN_NAMES[0]) { g.world_gain = v; }
    if let Some(v) = lib.base_constant(GAIN_NAMES[1]) { g.pe_gain = v; }
    if let Some(v) = lib.base_constant(GAIN_NAMES[2]) { g.pe_ref = v; }
    if let Some(v) = lib.base_constant(GAIN_NAMES[3]) { g.maturity_ref = v; }
    g
}

// ===== ① 世界模型匹配度 =====

/// 相似度 = 1 − 平均绝对差（两向量均在 0..1 → 结果在 0..1）。
pub fn closeness(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let mut acc = 0.0;
    for i in 0..4 {
        let d = (a[i] - b[i]).abs();
        acc += if d.is_finite() { d } else { 1.0 };
    }
    (1.0 - acc / 4.0).clamp(0.0, 1.0)
}

/// 世界模型匹配结果（**可解释**：局部/全局/成熟度/综合，便于结论携带依据）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldMatch {
    /// 与"该 key 对应条目"（当前页面/站点）的匹配度；世界模型里没有此 key 时为 `None`。
    pub local: Option<f64>,
    /// 与**全世界均值**的匹配度。
    pub global: f64,
    /// 世界模型成熟度 0..1（观测次数越多越成熟）。
    pub maturity: f64,
    /// **综合匹配度**：成熟度不足时向 0.5 收缩（证据不足 → 不改变结论）。
    pub score: f64,
}

/// **计算当前场域与世界模型（历史模式）的匹配度**。
///
/// - `key`：当前页面/站点在世界模型中的键（如 url 或 host）；`None` 则只看全局。
/// - 综合分向 0.5 收缩的依据是**成熟度**（`model.tick()`）：空模型 → 恰好 0.5 → 修正量 0。
pub fn match_to_world(
    field: &FieldReading,
    model: &WorldModel,
    key: Option<&str>,
    g: &Gains,
) -> WorldMatch {
    let f = field.to_array();
    let s = model.summary();
    let global = closeness(&f, &s.mean);
    let local = key.and_then(|k| {
        model
            .entries()
            .iter()
            .find(|e| e.key == k)
            .map(|e| closeness(&f, &e.mean))
    });
    let mref = if g.maturity_ref.is_finite() && g.maturity_ref > 0.0 {
        g.maturity_ref
    } else {
        MATURITY_REF_DEFAULT
    };
    let maturity = ((model.tick() as f64) / mref).clamp(0.0, 1.0);
    let raw = match local {
        Some(l) => LOCAL_WEIGHT * l + (1.0 - LOCAL_WEIGHT) * global,
        None => global,
    };
    let score = (0.5 + (raw - 0.5) * maturity).clamp(0.0, 1.0);
    WorldMatch { local, global, maturity, score }
}

// ===== ② 预测误差 =====

/// 预测误差 → 0..1 分（**单调递减**）：`pe=0 → 1`；`pe=pe_ref → 0.5`；`pe→∞ → 0`。
///
/// 用 `pe_ref / (pe_ref + pe)` 而非线性截断，好处是**无需设上限**、误差再大也只是趋 0，
/// 不会饱和成一刀切。
pub fn error_score(pe: f64, pe_ref: f64) -> f64 {
    let p = if pe.is_finite() && pe > 0.0 { pe } else { 0.0 };
    let r = if pe_ref.is_finite() && pe_ref > 0.0 { pe_ref } else { PE_REF_DEFAULT };
    let s = r / (r + p);
    if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.0 }
}

// ===== 证据 → 置信度修正 =====

/// 诊断证据（可选：缺哪项就不按哪项加权）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Evidence {
    /// 世界模型匹配度 0..1（由 [`match_to_world`] 得出）。
    pub world_match: Option<f64>,
    /// 预测误差（宿主实测；越小越"可预测"）。
    pub prediction_error: Option<f64>,
}

impl Evidence {
    /// 无证据（等价于 `default()`）。
    pub const NONE: Evidence = Evidence { world_match: None, prediction_error: None };

    pub fn is_empty(&self) -> bool {
        self.world_match.is_none() && self.prediction_error.is_none()
    }
}

/// 置信度修正结果（**既是计算结果，也是"结论携带的依据"**）。
#[derive(Clone, Debug, PartialEq)]
pub struct Adjustment {
    /// 修正前的确信度（仅由场域自身推导）。
    pub base: f64,
    /// 世界模型匹配度原值（若有）。
    pub world_match: Option<f64>,
    /// 世界模型带来的修正量（可正可负）。
    pub world_adjust: f64,
    /// 预测误差原值（若有）。
    pub prediction_error: Option<f64>,
    /// 预测误差归一化分 0..1（若有）。
    pub error_score: Option<f64>,
    /// 预测误差带来的修正量（可正可负）。
    pub pe_adjust: f64,
    /// 修正后的最终确信度（0..1）。
    pub final_confidence: f64,
    /// 人类可读依据（L6 据此呈现"为什么是这个确信度"）。
    pub note: String,
}

impl Adjustment {
    /// 证据是否真的改变了结论（用于判断"依据"是否需要随结论外发）。
    pub fn is_significant(&self) -> bool {
        self.world_adjust.abs() > 1e-9 || self.pe_adjust.abs() > 1e-9
    }
}

fn sign(d: f64) -> &'static str {
    if d > 1e-9 { "↑" } else if d < -1e-9 { "↓" } else { "=" }
}

/// **证据 → 置信度修正**（纯函数）。
///
/// 方向（要求的验收语义）：
/// - 世界模型匹配度高 → `world_adjust > 0`（置信度提高）；低 → `< 0`（降低）；
/// - 预测误差低 → `pe_adjust > 0`（提高）；高 → `< 0`（降低）。
pub fn adjust(base: f64, ev: &Evidence, g: &Gains) -> Adjustment {
    let base = if base.is_finite() { base.clamp(0.0, 1.0) } else { 0.0 };
    let mut note = String::new();

    let (wm, world_adjust) = match ev.world_match {
        Some(m) if m.is_finite() => {
            let m = m.clamp(0.0, 1.0);
            let d = g.world_gain * (2.0 * m - 1.0);
            note.push_str(&format!("世界模型匹配度 {m:.3} {}{:.3}", sign(d), d.abs()));
            (Some(m), d)
        }
        _ => (None, 0.0),
    };

    let (pe, es, pe_adjust) = match ev.prediction_error {
        Some(p) if p.is_finite() => {
            let s = error_score(p, g.pe_ref);
            let d = g.pe_gain * (2.0 * s - 1.0);
            if !note.is_empty() {
                note.push_str("；");
            }
            note.push_str(&format!("预测误差 {p:.4} → 误差分 {s:.3} {}{:.3}", sign(d), d.abs()));
            (Some(p.max(0.0)), Some(s), d)
        }
        _ => (None, None, 0.0),
    };

    let final_confidence = (base + world_adjust + pe_adjust).clamp(0.0, 1.0);
    Adjustment {
        base,
        world_match: wm,
        world_adjust,
        prediction_error: pe,
        error_score: es,
        pe_adjust,
        final_confidence,
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l5_quad::Quad;

    fn f(e: f64, w: f64, fi: f64, wi: f64) -> FieldReading {
        FieldReading { earth: e, water: w, fire: fi, wind: wi, confidence: 0.8 }
    }

    /// 建一个"稳定结构型"世界：多次看到同一页面（低张力、高结构）。
    fn mature_calm() -> WorldModel {
        let mut m = WorldModel::new();
        for _ in 0..10 {
            m.observe_page("h", "h/a", &f(0.4, 0.6, 0.1, 0.1), 1000);
        }
        m
    }

    #[test]
    fn closeness_is_bounded_and_symmetric() {
        let a = [0.0, 0.0, 0.0, 0.0];
        let b = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(closeness(&a, &a), 1.0);
        assert_eq!(closeness(&a, &b), 0.0);
        assert!((closeness(&a, &b) - closeness(&b, &a)).abs() < 1e-12, "对称");
        // 单分量差 0.4 → 平均差 0.1 → 0.9
        assert!((closeness(&[0.0; 4], &[0.4, 0.0, 0.0, 0.0]) - 0.9).abs() < 1e-12);
    }

    #[test]
    fn empty_world_gives_neutral_match() {
        let m = WorldModel::new();
        let r = match_to_world(&f(0.9, 0.1, 0.9, 0.9), &m, Some("h/a"), &Gains::default());
        assert_eq!(r.local, None);
        assert_eq!(r.score, 0.5, "空模型 → 中性 0.5（证据不足不改变结论）");
        assert!((r.maturity - 0.0).abs() < 1e-12);
    }

    #[test]
    fn match_high_when_field_looks_like_history() {
        let m = mature_calm();
        let like = match_to_world(&f(0.4, 0.6, 0.1, 0.1), &m, Some("h/a"), &Gains::default());
        let unlike = match_to_world(&f(0.05, 0.1, 0.95, 0.95), &m, Some("h/a"), &Gains::default());
        assert!(like.score > 0.9, "同形 → 高匹配 {}", like.score);
        assert!(unlike.score < 0.4, "异形 → 低匹配 {}", unlike.score);
        assert!(like.score > unlike.score);
        assert!((like.maturity - 1.0).abs() < 1e-12, "10 次观测已成熟");
    }

    #[test]
    fn immature_model_shrinks_toward_neutral() {
        let mut m = WorldModel::new();
        m.observe_page("h", "h/a", &f(0.4, 0.6, 0.1, 0.1), 1000); // tick=1 → maturity=1/8
        let r = match_to_world(&f(0.4, 0.6, 0.1, 0.1), &m, Some("h/a"), &Gains::default());
        assert!(r.score > 0.5 && r.score < 0.6, "证据不足 → 向 0.5 收缩：{}", r.score);
        assert!((r.maturity - 0.125).abs() < 1e-12);
    }

    #[test]
    fn error_score_is_monotonically_decreasing() {
        assert!((error_score(0.0, 0.05) - 1.0).abs() < 1e-12, "零误差 → 满分");
        assert!((error_score(0.05, 0.05) - 0.5).abs() < 1e-12, "等于参考 → 半分");
        assert!(error_score(0.1, 0.05) < 0.5);
        assert!(error_score(0.5, 0.05) < error_score(0.1, 0.05));
        let mut prev = 1.1;
        for i in 0..20 {
            let s = error_score(i as f64 * 0.05, 0.05);
            assert!(s <= prev, "必须单调不增");
            assert!((0.0..=1.0).contains(&s));
            prev = s;
        }
    }

    #[test]
    fn high_world_match_raises_confidence_low_lowers() {
        let g = Gains::default();
        let hi = adjust(0.6, &Evidence { world_match: Some(1.0), prediction_error: None }, &g);
        let mid = adjust(0.6, &Evidence { world_match: Some(0.5), prediction_error: None }, &g);
        let lo = adjust(0.6, &Evidence { world_match: Some(0.0), prediction_error: None }, &g);
        assert!(hi.final_confidence > mid.final_confidence, "匹配高 → 置信度高");
        assert!(mid.final_confidence > lo.final_confidence, "匹配低 → 置信度低");
        assert!(hi.world_adjust > 0.0 && lo.world_adjust < 0.0);
        assert!((mid.world_adjust).abs() < 1e-12, "中性匹配不修正");
        assert!(hi.note.contains("世界模型匹配度"), "依据须可读: {}", hi.note);
    }

    #[test]
    fn low_prediction_error_raises_confidence_high_lowers() {
        let g = Gains::default();
        let low = adjust(0.6, &Evidence { world_match: None, prediction_error: Some(0.005) }, &g);
        let refv = adjust(0.6, &Evidence { world_match: None, prediction_error: Some(g.pe_ref) }, &g);
        let high = adjust(0.6, &Evidence { world_match: None, prediction_error: Some(0.5) }, &g);
        assert!(low.final_confidence > refv.final_confidence, "误差低 → 置信度高");
        assert!(refv.final_confidence > high.final_confidence, "误差高 → 置信度低");
        assert!(low.pe_adjust > 0.0 && high.pe_adjust < 0.0);
        // 结论必须**携带**预测误差原值（发起人要求）
        assert_eq!(low.prediction_error, Some(0.005));
        assert!(low.note.contains("预测误差 0.0050"), "依据须含原值: {}", low.note);
    }

    #[test]
    fn no_evidence_changes_nothing() {
        let g = Gains::default();
        let a = adjust(0.75, &Evidence::NONE, &g);
        assert!(a.world_match.is_none() && a.prediction_error.is_none());
        assert!((a.final_confidence - 0.75).abs() < 1e-12, "无证据 → 置信度不变");
        assert!(!a.is_significant());
        assert!(a.note.is_empty());
    }

    #[test]
    fn confidence_stays_bounded_and_ignores_nan() {
        let g = Gains::default();
        let a = adjust(0.95, &Evidence { world_match: Some(1.0), prediction_error: Some(0.0) }, &g);
        assert!((0.0..=1.0).contains(&a.final_confidence), "上界 {}", a.final_confidence);
        let b = adjust(0.05, &Evidence { world_match: Some(0.0), prediction_error: Some(999.0) }, &g);
        assert!((0.0..=1.0).contains(&b.final_confidence), "下界 {}", b.final_confidence);
        let c = adjust(f64::NAN, &Evidence { world_match: Some(f64::NAN), prediction_error: Some(f64::NAN) }, &g);
        assert!((0.0..=1.0).contains(&c.final_confidence), "NaN 不得越界：{}", c.final_confidence);
    }

    #[test]
    fn gains_come_from_gene_library() {
        let mut lib = GeneLibrary::new();
        seed_into(&mut lib);
        let mut g = gains_of(&lib);
        assert!((g.pe_gain - PE_GAIN_DEFAULT).abs() < 1e-12);
        // 改库即改判据：把 pe_gain 翻倍 → 修正量翻倍
        lib.set_base_constant(GAIN_NAMES[1], 2.0 * PE_GAIN_DEFAULT, [1.0; 7]);
        let g2 = gains_of(&lib);
        assert!((g2.pe_gain - 2.0 * PE_GAIN_DEFAULT).abs() < 1e-12);
        // 未 seed 的库 → 回退默认
        let empty = GeneLibrary::new();
        g = gains_of(&empty);
        assert!((g.world_gain - WORLD_GAIN_DEFAULT).abs() < 1e-12);
    }

    #[test]
    fn interaction_enters_world_match() {
        // 交互也进世界模型（四元组折算成虚拟场域）→ 匹配度应受其影响
        let mut m = WorldModel::new();
        for _ in 0..10 {
            m.observe_interaction("quad", &Quad { tension: 0.9, calm: 0.1, liking: 0.5, safety: 0.2 });
        }
        let tense = // wind 与 quad_as_field 的口径一致：(1−平静)·0.5 + 喜欢·0.5 = 0.45 + 0.25 = 0.7
        match_to_world(&f(0.2, 0.1, 0.9, 0.7), &m, Some("act:quad"), &Gains::default());
        let calm = match_to_world(&f(0.9, 0.9, 0.1, 0.05), &m, Some("act:quad"), &Gains::default());
        assert!(tense.score > calm.score, "紧张世界里的紧张场更匹配：{} vs {}", tense.score, calm.score);
    }
}
