//! 斐波那契引擎 —— 以斐波那契比例（黄金分割）为节奏的变化模式调度内核。
//!
//! 概念：步长按斐波那契数列 / 黄金比例 (φ≈1.618) 递进的变化模式，
//! 对应自然生长、螺旋、松果与花瓣式的"非匀速但和谐"的生命感。
//!
//! ⚠️ **Phase 0 空壳**：占位 API 稳定，算法体待《数学规范白皮书》定稿后实现。
//! 注意：本引擎**不是**朴素斐波那契数列计算器，而是"斐波那契变化模式调度器"。

/// 斐波那契引擎。
///
/// 空壳阶段：`step` 为"直通"占位。**勿用于生产。**
#[derive(Debug, Clone, PartialEq)]
pub struct FibEngine {
    /// 最近一次输入的种子值。
    seed: f32,
}

impl FibEngine {
    /// 以初始种子构造引擎。
    pub const fn new(seed: f32) -> Self {
        Self { seed }
    }

    /// 读取当前种子。
    pub const fn seed(&self) -> f32 {
        self.seed
    }

    /// 推入一个种子并推进一步，返回本步输出。
    ///
    /// TODO(数学规范)：替换为"0 锚点 + 模糊饱和"定义下的斐波那契步进。
    pub fn step(&mut self, seed: f32) -> f32 {
        self.seed = seed;
        seed
    }
}

impl Default for FibEngine {
    fn default() -> Self {
        Self::new(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_holds_seed() {
        let e = FibEngine::new(1.0);
        assert_eq!(e.seed(), 1.0);
    }

    #[test]
    fn step_passthrough_placeholder() {
        let mut e = FibEngine::default();
        assert_eq!(e.step(0.618_034), 0.618_034);
        assert_eq!(e.seed(), 0.618_034);
    }
}
