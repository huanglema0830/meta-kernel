//! # 物态判定（State）
//!
//! 化学变化层第一环：按系统熵值判定当前"物态"——
//! 物理变化（线性叠加/力推动）之上，物态决定系统处于何种演化模式：
//!
//! | 物态 | 熵值 | 语义 |
//! |---|---|---|
//! | Solid 固态 | < 0.2 | 结构固化（强约束、低自由） |
//! | Liquid 液态 | 0.2 ≤ e < 0.5 | 可流动（结构松动、可混合） |
//! | Gas 气态 | 0.5 ≤ e < 0.8 | 自由组合（弱约束、高机动） |
//! | Plasma 等离子态 | ≥ 0.8 | 完全融合/创造态（旧结构解体、可化合出新） |
//!
//! 独立模块、无依赖：输入归一化香农熵即可判定（`state_of_entropy`），
//! 或直接对 `Pattern` 判定（取其 history 的归一化熵）。

use crate::ontology::Pattern;

/// 四态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Solid,
    Liquid,
    Gas,
    Plasma,
}

impl State {
    /// 中文标签（供界面显示）。
    pub const fn label_cn(self) -> &'static str {
        match self {
            State::Solid => "固态",
            State::Liquid => "液态",
            State::Gas => "气态",
            State::Plasma => "等离子态",
        }
    }

    /// 数值码（0-3，FFI/界面用）。
    pub const fn code(self) -> u32 {
        match self {
            State::Solid => 0,
            State::Liquid => 1,
            State::Gas => 2,
            State::Plasma => 3,
        }
    }
}

/// 阈值常量。
pub const SOLID_MAX: f32 = 0.2;
pub const LIQUID_MAX: f32 = 0.5;
pub const GAS_MAX: f32 = 0.8;

/// 按熵值判定物态（0 ≤ e ≤ 1）。
pub fn state_of_entropy(entropy: f32) -> State {
    let e = entropy.clamp(0.0, 1.0);
    if e < SOLID_MAX {
        State::Solid
    } else if e < LIQUID_MAX {
        State::Liquid
    } else if e < GAS_MAX {
        State::Gas
    } else {
        State::Plasma
    }
}

/// 归一化香农熵（8 桶 / log2(8)=3，输入为历史观测序列；空 → 1.0 真空）。
pub fn entropy_of_history(history: &[f64]) -> f32 {
    if history.is_empty() {
        return 1.0;
    }
    let mut bins = [0u64; 8];
    for v in history {
        let i = (((*v).clamp(0.0, 1.0) as f32) * 8.0) as usize;
        bins[i.min(7)] += 1;
    }
    let n = history.len() as f64;
    let h: f64 = bins
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / n;
            -p * p.log2()
        })
        .sum();
    ((h / 3.0).min(1.0)) as f32
}

/// 对模式判定物态（取 history 熵；history 为空退回元素强度熵，仍空 → 真空等离子）。
pub fn state_of(p: &Pattern) -> State {
    if !p.history.is_empty() {
        return state_of_entropy(entropy_of_history(&p.history));
    }
    if !p.elements.is_empty() {
        let hist: Vec<f64> = p.elements.iter().map(|e| e.intensity).collect();
        return state_of_entropy(entropy_of_history(&hist));
    }
    state_of_entropy(1.0) // 真空 = 等离子（创造态待化合）
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::Element;

    #[test]
    fn threshold_boundaries() {
        assert_eq!(state_of_entropy(0.0), State::Solid);
        assert_eq!(state_of_entropy(0.19), State::Solid);
        assert_eq!(state_of_entropy(0.2), State::Liquid);
        assert_eq!(state_of_entropy(0.49), State::Liquid);
        assert_eq!(state_of_entropy(0.5), State::Gas);
        assert_eq!(state_of_entropy(0.79), State::Gas);
        assert_eq!(state_of_entropy(0.8), State::Plasma);
        assert_eq!(state_of_entropy(1.0), State::Plasma);
    }

    #[test]
    fn labels_and_codes() {
        assert_eq!(State::Solid.label_cn(), "固态");
        assert_eq!(State::Plasma.label_cn(), "等离子态");
        assert_eq!(State::Solid.code(), 0);
        assert_eq!(State::Plasma.code(), 3);
    }

    #[test]
    fn pattern_state_from_history() {
        // 恒定历史 → 熵 0 → 固态
        let solid = Pattern::new(vec![Element::new(1, 0.5)]).with_history(vec![0.5; 32]);
        assert_eq!(state_of(&solid), State::Solid);
        // 剧烈随机 → 接近均匀 → 气态/等离子
        let chaotic = Pattern::new(vec![Element::new(1, 0.5)])
            .with_history((0..64).map(|i| ((i * 7) % 8) as f64 / 8.0).collect());
        let st = state_of(&chaotic);
        assert!(matches!(st, State::Gas | State::Plasma), "{st:?}");
    }

    #[test]
    fn entropy_normalized() {
        assert_eq!(entropy_of_history(&[]), 1.0);
        assert_eq!(entropy_of_history(&[0.5; 16]), 0.0);
        let h = entropy_of_history(&(0..64).map(|i| ((i * 3) % 8) as f64 / 8.0).collect::<Vec<_>>());
        assert!((0.0..=1.0).contains(&h), "{h}");
    }
}
