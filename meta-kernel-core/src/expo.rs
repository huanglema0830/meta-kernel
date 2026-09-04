//! 指数引擎 —— 以指数规律（加速 / 衰减）为变化模式的调度内核。
//!
//! 概念：变化率随状态指数放大的模式（如雪崩、复利、潮汐涨落的渐强段），
//! 是三种"生命感"变化模式中最有张力的一种。
//!
//! ⚠️ **Phase 0 空壳**：占位 API 稳定，算法体待《数学规范白皮书》定稿后实现。
//! 空壳阶段尤其注意：指数运算是溢出与 NaN 的高发区，Phase 1 将围绕
//! "10000 次迭代不溢出"开展专项测试。

/// 指数引擎。
///
/// 空壳阶段：`step` 为"直通"占位。**勿用于生产。**
#[derive(Debug, Clone, PartialEq)]
pub struct ExpoEngine {
    /// 最近一次输入的种子值。
    seed: f32,
}

impl ExpoEngine {
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
    /// TODO(数学规范)：替换为"0 锚点 + 模糊饱和"定义下的指数步进。
    pub fn step(&mut self, seed: f32) -> f32 {
        self.seed = seed;
        seed
    }
}

impl Default for ExpoEngine {
    fn default() -> Self {
        Self::new(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_holds_seed() {
        let e = ExpoEngine::new(-1.0);
        assert_eq!(e.seed(), -1.0);
    }

    #[test]
    fn step_passthrough_placeholder() {
        let mut e = ExpoEngine::default();
        assert_eq!(e.step(2.718_282), 2.718_282);
        assert_eq!(e.seed(), 2.718_282);
    }
}
