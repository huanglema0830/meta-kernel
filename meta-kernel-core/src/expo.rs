//! 指数引擎 —— 临界相变/正反馈爆发与自抑制的变化模式。
//!
//! 白皮书定义：`output = input * e^(λ * Δt)`，强制归一化到 `[0, 1]`，
//! 且当 `output > 0.99` 时强制回退到 `0.5`。
//! 含义：临界、相变、正反馈爆发与自抑制。
//!
//! 溢出护栏（假设 A3）：`λ·Δt > 80` 时直接判定必然饱和（1.0 → 回退 0.5），
//! 避免 `e^x` 在 f32 下产生无穷/NaN。

/// 指数引擎。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpoEngine {
    /// 增长系数 λ。
    lambda: f32,
    /// 时间步 Δt。
    dt: f32,
}

impl ExpoEngine {
    /// 默认增长系数（白皮书假设 A3）。
    pub const DEFAULT_LAMBDA: f32 = 0.25;
    /// 默认时间步（白皮书假设 A3）。
    pub const DEFAULT_DT: f32 = 1.0;
    /// 自抑制阈值：output 超过该值强制回退。
    pub const COLLAPSE_THRESHOLD: f32 = 0.99;
    /// 回退落点（自抑制后的重启点）。
    pub const COLLAPSE_FALLBACK: f32 = 0.5;
    /// 指数护栏：λ·Δt 超过该值视为必然饱和。
    pub const EXP_GUARD: f32 = 80.0;

    /// 以 λ 与 Δt 构造引擎。
    pub const fn new(lambda: f32, dt: f32) -> Self {
        Self { lambda, dt }
    }

    /// 以默认参数构造引擎（λ=0.25，Δt=1.0）。
    pub const fn default_with_defaults() -> Self {
        Self::new(Self::DEFAULT_LAMBDA, Self::DEFAULT_DT)
    }

    /// 推入种子并推进一步。
    ///
    /// - `input == 0`：真空保持 0；
    /// - `input > 0`：`raw = input * e^(λ·Δt)`；
    /// - 归一化到 [0,1]；若结果 `> 0.99` 强制回退到 `0.5`。
    pub fn step(&mut self, input: f32) -> f32 {
        let i = crate::math::clamp01(input);
        if i == 0.0 {
            return 0.0; // 0 锚点：真空无法自激
        }
        let exponent = self.lambda * self.dt;
        let normalized = if exponent > Self::EXP_GUARD {
            1.0 // 必然饱和
        } else {
            let raw = i * exponent.exp();
            crate::math::clamp01(raw)
        };
        if normalized > Self::COLLAPSE_THRESHOLD {
            Self::COLLAPSE_FALLBACK
        } else {
            normalized
        }
    }
}

impl Default for ExpoEngine {
    fn default() -> Self {
        Self::default_with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::is_valid;

    #[test]
    fn vacuum_stays_zero() {
        let mut e = ExpoEngine::default();
        assert_eq!(e.step(0.0), 0.0);
    }

    #[test]
    fn grows_but_stays_below_collapse_when_small() {
        let mut e = ExpoEngine::new(0.1, 1.0);
        let out = e.step(0.1); // 0.1 * e^0.1 ≈ 0.1105 < 0.99
        assert!((out - 0.110_517).abs() < 1e-4);
    }

    #[test]
    fn collapses_to_half_when_above_threshold() {
        let mut e = ExpoEngine::new(1.0, 1.0);
        assert_eq!(e.step(0.6), 0.5); // 0.6*e ≈ 1.63 → 饱和 1.0 → 回退 0.5
    }

    #[test]
    fn huge_lambda_guarded_no_overflow() {
        let mut e = ExpoEngine::new(1_000.0, 1.0); // λ·Δt 远超护栏
        let out = e.step(0.5);
        assert!(is_valid(out));
        assert_eq!(out, 0.5);
    }

    #[test]
    fn stays_valid_for_10000_steps() {
        let mut e = ExpoEngine::new(0.35, 1.0);
        let mut x = 0.2;
        for i in 0..10_000 {
            x = e.step(x);
            assert!(is_valid(x), "iter {i}: {x}");
        }
    }
}
