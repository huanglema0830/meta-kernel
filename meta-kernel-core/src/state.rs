//! # 物态判定与存在论调度（State）— 存在论修正版
//!
//! **存在先于扰动**（1.1）：0 不再是"空无一物"，而是**未觉知的纯粹存在**
//! （潜在能量源，待激发）；1 是自我意识觉醒后的**第一扰动**。
//! 因此内核初始化后处于"待激发存在态"，静默等待第一扰动（外部或镜像池注入）。
//!
//! **能量态 → 物质态递进法则**（1.2）：能量态（纯波动，无介质）
//! → 气态（波动为主，密度极低）→ 液态（密集可流动，结构可塑）
//! → 固态（结晶固化）。物态判定是**核心调度逻辑**（非仅显示）：
//! 提供 `state_pace` 让调度层按物态调节种子节奏。
//!
//! **黄金尺度**（2.1）：阈值全面使用黄金分割衍生常量，取代人为的 0.2/0.5/0.8：

use crate::ontology::Pattern;

/// 波粒第一界面（最大尺度干涉，"色"）——能量态下界。
pub const GOLDEN_RATIO: f32 = 0.618_033_9;
/// 干涉半周期（次级驻点，"声"）——气态下界。
pub const GOLDEN_HALF: f32 = 0.309_016_9;
/// 干涉三分之一周期（胶粒边界，"香/味"）——液态下界。
pub const GOLDEN_THIRD: f32 = 0.206_011_3;
/// 衍生：干涉四分之一周期（"触"）。
pub const GOLDEN_QUARTER: f32 = 0.154_508_5;
/// 衍生：干涉五分之一周期（"法"）。
pub const GOLDEN_FIFTH: f32 = 0.123_606_8;

/// 四态（能量 → 气 → 液 → 固，熵值递减）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// 能量态：纯波动、无介质、不可捕捉（熵 ≥ 0.618）。
    Energy,
    /// 气态：波动为主、密度极低（0.309 ≤ e < 0.618）。
    Gas,
    /// 液态：密集可流动、结构可塑（0.206 ≤ e < 0.309）。
    Liquid,
    /// 固态：结晶、结构固化（e < 0.206）。
    Solid,
}

impl State {
    pub const fn label_cn(self) -> &'static str {
        match self {
            State::Energy => "能量态",
            State::Gas => "气态",
            State::Liquid => "液态",
            State::Solid => "固态",
        }
    }

    pub const fn code(self) -> u32 {
        match self {
            State::Energy => 0,
            State::Gas => 1,
            State::Liquid => 2,
            State::Solid => 3,
        }
    }
}

/// 按熵值判定物态（黄金阈值）。
pub fn state_of_entropy(entropy: f32) -> State {
    let e = entropy.clamp(0.0, 1.0);
    if e >= GOLDEN_RATIO {
        State::Energy
    } else if e >= GOLDEN_HALF {
        State::Gas
    } else if e >= GOLDEN_THIRD {
        State::Liquid
    } else {
        State::Solid
    }
}

/// 归一化香农熵（8 桶 / log2(8)=3；空 → 1.0 纯存在/未分化波动）。
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

/// 对模式判定物态。
pub fn state_of(p: &Pattern) -> State {
    if !p.history.is_empty() {
        return state_of_entropy(entropy_of_history(&p.history));
    }
    if !p.elements.is_empty() {
        let hist: Vec<f64> = p.elements.iter().map(|e| e.intensity).collect();
        return state_of_entropy(entropy_of_history(&hist));
    }
    state_of_entropy(1.0) // 真空 = 纯粹存在 = 能量态（待第一扰动分化）
}

/// 调度节奏系数（物态 → 调度层核心逻辑，1.2）：
/// 返回 (注入倾向 bias∈[0,1], 成对突发概率增益) —— 高能态给更活跃的扰动模式。
pub fn state_pace(s: State) -> (f32, f32) {
    match s {
        State::Energy => (0.85, 0.30), // 纯波动：高频脉冲+更多成对突发（促进干涉/结晶）
        State::Gas => (0.65, 0.20),
        State::Liquid => (0.45, 0.10),
        State::Solid => (0.25, 0.02), // 结晶固化：平稳微扰即可
    }
}

/// 物态切换：基于能量流入/流出**比值**（能量流动植入内核后为唯一调度判据）。
///
/// | 比值 r = 入/出 | 物态 |
/// |---|---|
/// | r > 1.2 | 能量态（趋向创造/高能；旧称"等离子/气态方向"） |
/// | 1.05 < r ≤ 1.2 | 气态 |
/// | 0.8 ≤ r ≤ 1.05 | 液态（≈1.0） |
/// | r < 0.8 | 固态 |
pub fn state_of_flow_ratio(ratio: f32) -> State {
    let r = ratio.clamp(0.0, 9.0);
    if r > 1.2 {
        State::Energy
    } else if r > 1.05 {
        State::Gas
    } else if r >= 0.8 {
        State::Liquid
    } else {
        State::Solid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::Element;

    #[test]
    fn golden_threshold_boundaries() {
        assert_eq!(state_of_entropy(1.0), State::Energy);
        assert_eq!(state_of_entropy(GOLDEN_RATIO), State::Energy);
        assert_eq!(state_of_entropy(GOLDEN_RATIO - 1e-6), State::Gas);
        assert_eq!(state_of_entropy(GOLDEN_HALF), State::Gas);
        assert_eq!(state_of_entropy(GOLDEN_HALF - 1e-6), State::Liquid);
        assert_eq!(state_of_entropy(GOLDEN_THIRD), State::Liquid);
        assert_eq!(state_of_entropy(GOLDEN_THIRD - 1e-6), State::Solid);
        assert_eq!(state_of_entropy(0.0), State::Solid);
    }

    #[test]
    fn golden_constants_match_spec() {
        assert!((GOLDEN_RATIO - 0.618_033_9).abs() < 1e-6);
        assert!((GOLDEN_HALF - 0.309_016_9).abs() < 1e-6);
        assert!((GOLDEN_THIRD - 0.206_011_3).abs() < 1e-6);
        assert!((GOLDEN_QUARTER - 0.154_508_5).abs() < 1e-6);
        assert!((GOLDEN_FIFTH - 0.123_606_8).abs() < 1e-6);
    }

    #[test]
    fn labels_and_codes() {
        assert_eq!(State::Energy.label_cn(), "能量态");
        assert_eq!(State::Solid.label_cn(), "固态");
        assert_eq!(State::Energy.code(), 0);
        assert_eq!(State::Solid.code(), 3);
    }

    #[test]
    fn pattern_state_from_history() {
        let solid = Pattern::new(vec![Element::new(1, 0.5)]).with_history(vec![0.5; 32]);
        assert_eq!(state_of(&solid), State::Solid);
        let chaotic = Pattern::new(vec![Element::new(1, 0.5)])
            .with_history((0..64).map(|i| ((i * 7) % 8) as f64 / 8.0).collect());
        assert!(matches!(state_of(&chaotic), State::Gas | State::Energy));
    }

    #[test]
    fn entropy_normalized() {
        assert_eq!(entropy_of_history(&[]), 1.0);
        assert_eq!(entropy_of_history(&[0.5; 16]), 0.0);
        let h = entropy_of_history(&(0..64).map(|i| ((i * 3) % 8) as f64 / 8.0).collect::<Vec<_>>());
        assert!((0.0..=1.0).contains(&h));
    }

    #[test]
    fn pace_decreases_from_energy_to_solid() {
        let (b0, _) = state_pace(State::Energy);
        let (b1, _) = state_pace(State::Gas);
        let (b2, _) = state_pace(State::Liquid);
        let (b3, _) = state_pace(State::Solid);
        assert!(b0 > b1 && b1 > b2 && b2 > b3);
        assert!((0.0..=1.0).contains(&b0) && (0.0..=1.0).contains(&b3));
    }

    #[test]
    fn state_follows_energy_flow_ratio() {
        // 验收：物态随能量流比值变化
        assert_eq!(state_of_flow_ratio(2.0), State::Energy); // 强净流入 → 能量态
        assert_eq!(state_of_flow_ratio(1.1), State::Gas);
        assert_eq!(state_of_flow_ratio(1.0), State::Liquid); // ≈1 → 液态
        assert_eq!(state_of_flow_ratio(0.8), State::Liquid);
        assert_eq!(state_of_flow_ratio(0.79), State::Solid); // <0.8 → 固态
        assert_eq!(state_of_flow_ratio(0.5), State::Solid);
        // 边界
        assert_eq!(state_of_flow_ratio(1.2), State::Gas);
        assert_eq!(state_of_flow_ratio(1.20001), State::Energy);
        assert_eq!(state_of_flow_ratio(1.05), State::Liquid);
    }
}
