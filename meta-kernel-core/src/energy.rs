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
