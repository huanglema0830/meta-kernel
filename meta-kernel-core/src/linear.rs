//! 线性引擎 —— 等步长变化模式的调度内核。
//!
//! 概念：以恒定步长推进的状态变化器，是三种"生命感"变化模式中最平稳的一种
//! （对应呼吸、节拍器、匀速心跳）。
//!
//! ⚠️ **Phase 0 空壳**：本模块仅提供稳定的占位 API，数值语义待
//! 《数学规范白皮书》（0 锚点 + 模糊饱和运算）定稿后实现，见 TODO(数学规范)。

/// 线性引擎。
///
/// 空壳阶段：`step` 为"直通"占位——输入什么种子就返回什么，尚未做任何
/// 锚定/饱和运算。**勿用于生产。**
#[derive(Debug, Clone, PartialEq)]
pub struct LinearEngine {
    /// 最近一次输入的种子值。
    seed: f32,
}

impl LinearEngine {
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
    /// TODO(数学规范)：替换为"0 锚点 + 模糊饱和"定义下的线性步进。
    pub fn step(&mut self, seed: f32) -> f32 {
        self.seed = seed;
        seed
    }
}

impl Default for LinearEngine {
    fn default() -> Self {
        Self::new(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_holds_seed() {
        let e = LinearEngine::new(0.5);
        assert_eq!(e.seed(), 0.5);
    }

    #[test]
    fn step_passthrough_placeholder() {
        let mut e = LinearEngine::default();
        assert_eq!(e.step(1.25), 1.25);
        assert_eq!(e.seed(), 1.25);
    }
}
