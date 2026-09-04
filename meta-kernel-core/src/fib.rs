//! 斐波那契引擎 —— 自相似递归的"生长"变化模式。
//!
//! 白皮书定义：`output = (a + b) * 0.5`，其中 `a`、`b` 是前两次输出值，
//! 强制归一化到 `[0, 1]`。含义：生长、自相似递归、基于历史的自我参照。
//!
//! 0 锚点语义（假设 A2）：引擎初始为真空 `(a, b) = (0, 0)`，`step` 返回 0；
//! 首次收到正输入时视其为**第一扰动（1）**并点燃递归，之后 `a、b` 纯由
//! 历史输出滚动，不再读取输入。

/// 斐波那契引擎（二阶自递归）。
#[derive(Debug, Clone, PartialEq)]
pub struct FibEngine {
    /// 最近一次输出。
    a: f32,
    /// 上上次输出。
    b: f32,
    /// 是否已被第一扰动点燃。
    primed: bool,
}

impl FibEngine {
    /// 构造引擎（0 锚点真空态）。
    pub const fn new() -> Self {
        Self { a: 0.0, b: 0.0, primed: false }
    }

    /// 当前最近一次输出（真空态为 0）。
    pub const fn latest(&self) -> f32 {
        self.a
    }

    /// 推进一步。
    ///
    /// - 真空态且 `input > 0`：以 `input` 点燃（第一扰动），返回 `input`；
    /// - 真空态且 `input == 0`：保持真空，返回 `0.0`；
    /// - 已点燃：`output = (a + b) * 0.5`，滚动历史，返回 output。
    pub fn step(&mut self, input: f32) -> f32 {
        if !self.primed {
            let i = crate::math::clamp01(input);
            if i > 0.0 {
                self.a = i;
                self.b = 0.0;
                self.primed = true;
            }
            return self.a;
        }
        let out = ((self.a + self.b) * 0.5).clamp(0.0, 1.0);
        self.b = self.a;
        self.a = out;
        out
    }

    /// 回到 0 锚点真空态。
    pub fn reset_to_anchor(&mut self) {
        self.a = 0.0;
        self.b = 0.0;
        self.primed = false;
    }
}

impl Default for FibEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::is_valid;

    #[test]
    fn anchor_vacuum_stays_zero() {
        let mut e = FibEngine::new();
        assert_eq!(e.step(0.0), 0.0);
        assert_eq!(e.step(0.0), 0.0);
    }

    #[test]
    fn first_positive_input_ignites() {
        let mut e = FibEngine::new();
        assert_eq!(e.step(1.0), 1.0); // 第一扰动
    }

    #[test]
    fn recurrence_sequence_matches_formula() {
        let mut e = FibEngine::new();
        // 点燃：1 → (1+0)/2=0.5 → (0.5+1)/2=0.75 → (0.75+0.5)/2=0.625
        let seq: Vec<f32> = (0..4).map(|_| e.step(1.0)).collect();
        assert!((seq[0] - 1.0).abs() < 1e-6);
        assert!((seq[1] - 0.5).abs() < 1e-6);
        assert!((seq[2] - 0.75).abs() < 1e-6);
        assert!((seq[3] - 0.625).abs() < 1e-6);
    }

    #[test]
    fn stays_bounded_for_10000_steps() {
        let mut e = FibEngine::new();
        e.step(1.0); // 点燃
        for i in 0..10_000 {
            let out = e.step(0.0); // 点燃后输入被忽略，纯自参照
            assert!(is_valid(out), "iter {i}: {out}");
        }
        // 收敛区间应在 (0,1) 内且非零（递归未被抽干）
        assert!(e.latest() > 0.0);
    }
}
