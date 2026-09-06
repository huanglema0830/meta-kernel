//! # 能量层级判定（Energy）
//!
//! 将 0-10 元标尺与"能量频率/生命力"挂钩，判断任何模式的**正向性/生命力**。
//! 处置规则（发起人定）：
//!
//! | 活力指数 e | 处置 |
//! |---|---|
//! | e ≤ 0.2 | 直接分解到胶粒（层级 1） |
//! | 0.2 < e ≤ 0.5 | 进入"拆解-分析-重编"循环 |
//! | 0.5 < e ≤ 0.8 | 保留结构，标记"观察中" |
//! | e > 0.8 | 直接纳入正源库 |

use crate::ontology::{self, Element, Pattern};
use crate::sanitizer::finalize;

/// 各处置档的阈值。
pub const LOW_BAND: f64 = 0.2;
pub const MID_BAND: f64 = 0.5;
pub const HIGH_BAND: f64 = 0.8;

/// 处置决议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 活力极低 → 直接分解到胶粒。
    DecomposeToGranules,
    /// 中低 → 进入拆解-分析-重编循环。
    RecycleLoop,
    /// 中高 → 保留结构，观察中。
    Observe,
    /// 高 → 纳入正源库/功德池。
    Adopt,
}

/// 计算模式的生命活力指数 e ∈ [0,1]。
///
/// 口径：对 0-10 各层得分做**上倾加权**——高层级（结构化/完整呈现）权重更高，
/// 纯真空（仅黑）接近 0：
/// `e = Σ(w_l · s_l) / Σw`，其中 `w = [.05,.05,.10,.15,.15,.15,.10,.10,.10,.05,.30]`。
pub fn energy_level_evaluate(p: &Pattern) -> f64 {
    let s = ontology::analyze(p);
    let weights = [0.05, 0.05, 0.10, 0.15, 0.15, 0.15, 0.10, 0.10, 0.10, 0.05, 0.30];
    let w_sum: f64 = weights.iter().sum::<f64>();
    let num: f64 = s.iter().zip(weights.iter()).map(|(x, w)| x * w).sum();
    finalize((num / w_sum) as f32) as f64
}

/// 依据活力指数给出处置决议。
pub fn verdict_for(energy: f64) -> Verdict {
    if energy <= LOW_BAND {
        Verdict::DecomposeToGranules
    } else if energy <= MID_BAND {
        Verdict::RecycleLoop
    } else if energy <= HIGH_BAND {
        Verdict::Observe
    } else {
        Verdict::Adopt
    }
}

/// 负模式处置主函数：返回处置后应进入流水线的元素。
///
/// - `e ≤ 0.2` → 拆解到层级 1（胶粒）；
/// - `0.2 < e ≤ 0.5` → 拆解到层级 3（供"拆解-分析-重编"循环入口）；
/// - `0.5 < e ≤ 0.8` → 保留原结构（观察中，调用方只读）；
/// - `e > 0.8` → 保留原结构（调用方纳入正源库）。
pub fn collapse_negative(score: f64, p: &Pattern) -> Vec<Element> {
    match verdict_for(score) {
        Verdict::DecomposeToGranules => ontology::decompose(p, 1),
        Verdict::RecycleLoop => ontology::decompose(p, 3),
        Verdict::Observe | Verdict::Adopt => p.elements.clone(),
    }
}

// =====================================================================
// 能量池（Energy Pool）— 内核真实能量流状态源
// =====================================================================
//
// "能量流动"是内核的真实状态（不再是可视化参数）：
// - absorb()：能量流入（0 锚点/外部输入/镜像池回注）→ 滚动入流；
// - consume()：能量流出（摩擦/引擎耗散/输出沉降）→ 滚动出流；
// - ratio()：入/出比值，驱动物态判定（state_of_flow）与化合吸收率。

/// 能量流滚动衰减（最近流量权重，0<decay<1）。
pub const FLOW_DECAY: f32 = 0.9;
/// 比值防除零。
pub const FLOW_EPS: f32 = 1e-4;
/// 能量储备被动耗散（每 tick 保留率；≈1.5% 漏失，模拟摩擦/热沉）。
pub const DISSIPATION_DECAY: f32 = 0.985;
/// 自然回归衰减率 λ（e^(-λt) 指数衰减；每次响应后调用一次）。
pub const DECAY_LAMBDA: f32 = 0.1;
/// 自然回归步长 t。
pub const DECAY_STEP: f32 = 1.0;
/// 视为真正 0 的阈值（低于即置零，避免浮点尾巴）。
pub const DECAY_EPS: f32 = 1e-6;

/// 能量池：滚动入/出流累积器 + 真实能量储备。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyPool {
    /// 滚动能量流入（absorbed 口径）。
    pub flow_in: f32,
    /// 滚动能量流出（spent 口径）。
    pub flow_out: f32,
    /// 真实能量储备（库存）：吸收累积、消耗扣减、每 tick 被动耗散。
    pub stored: f32,
}

impl Default for EnergyPool {
    fn default() -> Self {
        Self::new()
    }
}

impl EnergyPool {
    pub fn new() -> Self {
        Self { flow_in: 0.0, flow_out: 0.0, stored: 0.0 }
    }

    /// 能量流入（0-1 归一；来自注入/锚点回注）。
    /// 同时累加入真实储备（库存上限 1.0）。
    pub fn absorb(&mut self, energy: f32) {
        let e = energy.clamp(0.0, 1.0);
        self.flow_in = self.flow_in * FLOW_DECAY + e;
        self.stored = (self.stored + e).min(1.0);
    }

    /// 能量流出（耗散；输出活动回落时产生摩擦消耗）。
    /// 同时扣减真实储备（库存下限 0.0）。
    pub fn consume(&mut self, energy: f32) {
        let e = energy.clamp(0.0, 1.0);
        self.flow_out = self.flow_out * FLOW_DECAY + e;
        self.stored = (self.stored - e).max(0.0);
    }

    /// 已吸收能量（入流现值，化合时读取此值，非模拟）。
    pub fn absorbed(&self) -> f32 {
        self.flow_in
    }

    /// 已耗散能量（出流现值）。
    pub fn spent(&self) -> f32 {
        self.flow_out
    }

    /// 真实能量储备（库存现值，∈[0,1]）。
    pub fn stored(&self) -> f32 {
        self.stored
    }

    /// 被动耗散：每个调度 tick 调用一次，储备按 DISSIPATION_DECAY 漏失。
    /// 模拟未输出活动时的摩擦/热沉；不影响滚动入/出流比值（状态判据不变）。
    pub fn dissipate(&mut self) {
        self.stored = (self.stored * DISSIPATION_DECAY).clamp(0.0, 1.0);
    }

    /// 入/出比（∞ 有界化：出流为 0 时视为大流量比 9）。
    pub fn ratio(&self) -> f32 {
        let out = self.flow_out.max(FLOW_EPS);
        let r = (self.flow_in + FLOW_EPS) / out;
        if r.is_finite() { r.min(9.0) } else { 9.0 }
    }

    /// 自然回归：响应后储备按 **e^(-λt)** 指数衰减（非硬复位）。
    ///
    /// 五戒·不饮酒的落点：只允许加法 / ×0.618 拆解 / 本 e 衰减，不做其他算法。
    /// 与 [`Self::dissipate`] 并存——dissipate 是每 tick 的被动漏失（摩擦/热沉，
    /// 状态判据不受影响）；natural_return 是"响应之后"的显式回归（波峰回落）。
    /// 不影响滚动入/出流比值（状态判据不变）；低于阈值即视为真正 0。
    pub fn natural_return(&mut self) {
        let k = (-DECAY_LAMBDA * DECAY_STEP).exp();
        self.stored = (self.stored * k).clamp(0.0, 1.0);
        if self.stored < DECAY_EPS {
            self.stored = 0.0;
        }
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;

    #[test]
    fn ratio_responds_to_in_and_out() {
        let mut p = EnergyPool::new();
        assert!((p.ratio() - 1.0).abs() < 1e-3, "空池比≈1: {}", p.ratio());
        p.absorb(0.5);
        p.absorb(0.5);
        assert!(p.ratio() > 1.0, "净流入比>1: {}", p.ratio());
        for _ in 0..40 {
            p.absorb(0.02);
            p.consume(0.5);
        }
        assert!(p.ratio() < 1.0, "净流出比<1: {}", p.ratio());
    }

    #[test]
    fn flow_is_rolling_not_cumulative() {
        let mut p = EnergyPool::new();
        for _ in 0..100 {
            p.absorb(1.0);
        }
        // 滚动入流饱和于约 1/(1-decay) 上限附近 → 不为累计无限大
        assert!(p.absorbed() <= 10.5, "滚动有界: {}", p.absorbed());
        assert!(p.absorbed() > 9.0, "接近稳态: {}", p.absorbed());
    }

    #[test]
    fn stored_accumulates_on_absorb_and_depletes_on_consume() {
        let mut p = EnergyPool::new();
        assert_eq!(p.stored(), 0.0, "空池储备为 0");
        p.absorb(0.5);
        p.absorb(0.5);
        assert!((p.stored() - 1.0).abs() < 1e-6, "两次吸收≈1.0: {}", p.stored());
        p.consume(0.3);
        assert!((p.stored() - 0.7).abs() < 1e-6, "消耗扣减: {}", p.stored());
        // 储备有界 [0,1]
        for _ in 0..100 {
            p.absorb(1.0);
        }
        assert_eq!(p.stored(), 1.0, "储备封顶 1.0");
        for _ in 0..100 {
            p.consume(1.0);
        }
        assert_eq!(p.stored(), 0.0, "储备兜底 0.0");
    }

    #[test]
    fn dissipation_leaks_stored_reserve_over_ticks() {
        let mut p = EnergyPool::new();
        for _ in 0..50 {
            p.absorb(0.8);
        }
        let before = p.stored();
        assert!(before > 0.5, "吸收后储备充足: {before}");
        // 停止注入，仅被动耗散
        for _ in 0..200 {
            p.dissipate();
        }
        let after = p.stored();
        assert!(after < before * 0.5, "耗散应显著漏失: {before}→{after}");
        assert!(after >= 0.0 && after <= 1.0, "储备仍界内: {after}");
        // 滚动入/出流比值不受耗散影响（状态判据不变）：高入流下比值仍高
        assert!(p.ratio() > 1.0, "耗散不改变入/出比值: {}", p.ratio());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::Element;

    fn e(l: u8, v: f64) -> Element {
        Element::new(l, v)
    }

    #[test]
    fn vacuum_is_nearly_zero_vitality() {
        let p = Pattern::default();
        let e = energy_level_evaluate(&p);
        assert!(e < 0.15, "真空活力应极低: {e}");
        assert_eq!(verdict_for(e), Verdict::DecomposeToGranules);
    }

    #[test]
    fn rich_structure_scores_high_vitality() {
        // 含互补对(2层)、三元闭合(3层)、重复自参照、多子系统、单调+稳定形态历史
        let p = Pattern::new(vec![
            e(2, 0.4),
            e(2, 0.6),
            e(3, 0.2),
            e(3, 0.5),
            e(3, 0.8),
            e(5, 0.5),
            e(5, 0.5),
            e(8, 0.9),
        ])
        .with_history(vec![0.1, 0.101, 0.102, 0.103, 0.104, 0.105]);
        let ev = energy_level_evaluate(&p);
        assert!(ev > 0.8, "近白结构活力应高: {ev}");
        assert_eq!(verdict_for(ev), Verdict::Adopt);
    }

    #[test]
    fn collapse_maps_bands() {
        let p = Pattern::new(vec![e(6, 0.8)]);
        // 高活力（近白结构）→ 保留结构
        let hp = Pattern::new(vec![
            e(2, 0.4),
            e(2, 0.6),
            e(3, 0.2),
            e(3, 0.5),
            e(3, 0.8),
            e(5, 0.5),
            e(5, 0.5),
            e(8, 0.9),
        ])
        .with_history(vec![0.1, 0.101, 0.102, 0.103, 0.104, 0.105]);
        let high = energy_level_evaluate(&hp);
        assert!(high > 0.8, "近白结构活力应>0.8: {high}");
        assert_eq!(collapse_negative(high, &p), p.elements);

        // 极低 → 拆到胶粒（level<=1）
        let low = collapse_negative(0.1, &p);
        assert!(low.iter().all(|x| x.level <= 1));

        // 中低 → 拆到循环入口层（level<=3）
        let mid = collapse_negative(0.35, &p);
        assert!(mid.iter().all(|x| x.level <= 3));
        assert!(mid.iter().any(|x| x.level >= 1));
    }

    #[test]
    fn verdict_thresholds() {
        assert_eq!(verdict_for(0.1), Verdict::DecomposeToGranules);
        assert_eq!(verdict_for(0.35), Verdict::RecycleLoop);
        assert_eq!(verdict_for(0.65), Verdict::Observe);
        assert_eq!(verdict_for(0.9), Verdict::Adopt);
    }
}

#[cfg(test)]
mod decay_tests {
    use super::*;

    #[test]
    fn natural_return_decays_not_hard_resets() {
        let mut p = EnergyPool::new();
        p.absorb(0.8);
        let before = p.stored();
        p.natural_return();
        let after = p.stored();
        // e^(-0.1) ≈ 0.9048 → 0.8 → ≈0.7239；不是硬复位为 0，也不是不变
        let expect = before * (-DECAY_LAMBDA * DECAY_STEP).exp();
        assert!((after - expect).abs() < 1e-5, "应按 e^(-λt) 衰减: {after} vs {expect}");
        assert!(after > 0.0 && after < before, "自然衰减而非硬复位/回涨");
    }

    #[test]
    fn decay_trend_follows_exponential() {
        let mut p = EnergyPool::new();
        p.absorb(1.0);
        let k = (-DECAY_LAMBDA * DECAY_STEP).exp();
        let mut prev = p.stored();
        let mut ratios_ok = true;
        // e^(-0.1·200) ≈ 2e-9 < 1e-6 → 足够多步必触底归零
        for _ in 0..200 {
            p.natural_return();
            let cur = p.stored();
            if prev > DECAY_EPS {
                let r = cur / prev;
                ratios_ok &= (r - k).abs() < 1e-5;
            }
            prev = cur;
        }
        assert!(ratios_ok, "每步衰减比应恒为 e^(-λt)");
        // 足够多步后触底为真正 0（先衰减、后归零，非一步硬复位）
        assert_eq!(p.stored(), 0.0, "低于阈值后应置 0");
    }

    #[test]
    fn natural_return_leaves_flow_ratio_untouched() {
        // 状态判据（入/出比值）不受自然回归影响
        let mut p = EnergyPool::new();
        p.absorb(0.6);
        let _ = p.consume(0.2);
        let ratio_before = p.ratio();
        for _ in 0..10 {
            p.natural_return();
        }
        assert!((p.ratio() - ratio_before).abs() < 1e-6, "比值判据不得被回归扰动");
    }
}
