//! 线性引擎 —— 等步长"平稳消耗"变化模式。
//!
//! 白皮书定义：`output = input + 0.01`，强制归一化到 `[0, 1]`。
//! 含义：惯性、恒常、可预测的平稳消耗。
//!
//! 实现采用**饱和加法**：`output = input ⊕ 0.01 = min(1.0, input + 0.01)`。

/// 线性引擎。
///
/// 无内部状态：输出仅由输入决定（纯函数式变化模式）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearEngine;

impl LinearEngine {
    /// 每次步进固定的推进量（公式中的 0.01）。
    pub const INCREMENT: f32 = 0.01;

    /// 构造引擎（状态无关）。
    pub const fn new() -> Self {
        Self
    }

    /// 推入种子并推进一步：`output = input ⊕ 0.01`。
    pub fn step(&mut self, input: f32) -> f32 {
        crate::math::sat_add(input, Self::INCREMENT)
    }
}

impl Default for LinearEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::is_valid;

    #[test]
    fn step_adds_increment() {
        let mut e = LinearEngine::new();
        assert_eq!(e.step(0.5), 0.51);
    }

    #[test]
    fn saturates_at_one() {
        let mut e = LinearEngine::new();
        assert_eq!(e.step(0.99), 1.0);
        assert_eq!(e.step(1.0), 1.0);
    }

    #[test]
    fn anchor_zero_restarts_from_increment() {
        let mut e = LinearEngine::new();
        assert_eq!(e.step(0.0), 0.01);
    }

    #[test]
    fn stays_in_unit_interval_for_10000_steps() {
        let mut e = LinearEngine::default();
        for i in 0..10_000 {
            let input = if i % 100 == 0 { 0.0 } else { 0.8 };
            let out = e.step(input);
            assert!(is_valid(out), "iter {i}: {out}");
        }
    }
}
