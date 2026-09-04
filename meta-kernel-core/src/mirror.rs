//! 镜像池 —— 摩擦源（Mirror Pool）。
//!
//! 角色（假设 A5，见 docs/MATH_SPEC.md）：
//! - **镜像回显**：观察每次系统活动（种子），将其存入回显池；
//! - **摩擦衰减**：每次回显以摩擦系数 μ（默认 0.95）衰减，能量渐耗（摩擦源）；
//! - **真空重启**：回显耗尽且连续 `stall_limit` 个 tick 无外部活动时，
//!   从池内注入第一扰动 `1.0`（0 锚点机制：第一扰动可来自内部镜像池）。
//!
//! 借此系统具备自持续节律与自恢复能力：活动 → 回显 → 衰减 → 耗尽 → 重启。

use std::collections::VecDeque;

/// 镜像池（摩擦源）。
#[derive(Debug, Clone)]
pub struct MirrorPool {
    /// 回显池（FIFO，容量受限）。
    echoes: VecDeque<f32>,
    /// 容量。
    cap: usize,
    /// 摩擦系数 μ（每次回显的能量衰减）。
    mu: f32,
    /// 连续无外部活动的 tick 计数。
    stall: u32,
    /// 真空重启阈值。
    stall_limit: u32,
    /// 回显次数。
    pub reflections: u64,
    /// 真空重启（第一扰动注入）次数。
    pub kicks: u64,
}

impl MirrorPool {
    /// 默认容量。
    pub const DEFAULT_CAP: usize = 16;
    /// 默认摩擦系数 μ。
    pub const DEFAULT_MU: f32 = 0.95;
    /// 默认重启阈值。
    pub const DEFAULT_STALL_LIMIT: u32 = 5;
    /// 回显低于该值视为能量耗尽（不再回池，等待真空重启）。
    pub const MIN_ECHO: f32 = 1e-6;

    /// 以默认参数构造（容量 16，μ=0.95，stall_limit=5）。
    pub fn new() -> Self {
        Self::with_params(Self::DEFAULT_CAP, Self::DEFAULT_MU, Self::DEFAULT_STALL_LIMIT)
    }

    /// 以指定参数构造。
    pub fn with_params(cap: usize, mu: f32, stall_limit: u32) -> Self {
        assert!(cap > 0 && (0.0..=1.0).contains(&mu) && stall_limit >= 1);
        Self {
            echoes: VecDeque::with_capacity(cap),
            cap,
            mu,
            stall: 0,
            stall_limit,
            reflections: 0,
            kicks: 0,
        }
    }

    /// 观察一次系统活动（0-1 种子）并存入回显池。
    pub fn observe(&mut self, activity: f32) {
        let a = crate::math::clamp01(activity);
        if self.echoes.len() == self.cap {
            self.echoes.pop_front();
        }
        self.echoes.push_back(a);
        self.stall = 0; // 有活动即解除停滞
    }

    /// 驱动一个 tick。
    ///
    /// - `external`：本 tick 的外部种子（有则观察之）；
    /// - 返回本 tick 应从池内产生的种子：
    ///   回显（摩擦衰减后重放）、真空重启（1.0）或 None（静默）。
    pub fn tick(&mut self, external: Option<f32>) -> Option<f32> {
        match external {
            Some(s) => self.observe(s),
            None => self.stall += 1,
        }

        // 1) 有回显 → 摩擦衰减后重放（借旧还新，池内容保持）
        if let Some(e) = self.echoes.pop_front() {
            let r = e * self.mu;
            if r > Self::MIN_ECHO {
                self.echoes.push_back(r);
                self.reflections += 1;
                return Some(r);
            }
            // 能量耗尽：不回池，落入下方真空重启判定
        }

        // 2) 回显耗尽（或为空）且停滞超阈 → 真空重启（第一扰动来自内部镜像池）
        if self.stall >= self.stall_limit {
            self.stall = 0;
            self.kicks += 1;
            self.observe(1.0); // 重启即观察，进入新的生命周期
            return Some(1.0);
        }

        None
    }
}

impl Default for MirrorPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::is_valid;

    #[test]
    fn reflects_with_friction_decay() {
        let mut p = MirrorPool::new();
        p.observe(1.0);
        let r1 = p.tick(None).expect("first echo");
        assert!((r1 - 0.95).abs() < 1e-6);
        let r2 = p.tick(None).expect("second echo");
        assert!((r2 - 0.9025).abs() < 1e-6); // 摩擦逐级衰减
        assert_eq!(p.reflections, 2);
        assert_eq!(p.kicks, 0);
    }

    #[test]
    fn vacuum_restart_injects_first_perturbation() {
        let mut p = MirrorPool::with_params(4, 0.95, 3);
        assert_eq!(p.tick(None), None);
        assert_eq!(p.tick(None), None);
        assert_eq!(p.tick(None), Some(1.0)); // 停滞达阈 → 真空重启
        assert_eq!(p.kicks, 1);
    }

    #[test]
    fn echoes_decay_to_exhaustion_then_restart() {
        let mut p = MirrorPool::with_params(4, 0.5, 3);
        p.observe(1.0);
        // 单条回显以 μ=0.5 衰减：0.5,0.25,... 直至 ≤ MIN_ECHO 后耗尽
        let mut ticks = 0u32;
        while p.kicks == 0 {
            p.tick(None);
            ticks += 1;
            assert!(ticks < 10_000, "echo never exhausted");
        }
        assert!(p.reflections > 0);
        assert_eq!(p.kicks, 1);
    }

    #[test]
    fn echoes_stay_valid_for_10000_ticks() {
        let mut p = MirrorPool::new();
        // 阶段 A：活跃期（周期脉冲喂入）——验证数值合法性与回显
        for i in 0..3000 {
            let ext = if i % 50 == 0 { Some(0.8) } else { None };
            if let Some(e) = p.tick(ext) {
                assert!(is_valid(e), "iter {i}: {e}");
            }
        }
        assert!(p.reflections > 0, "no reflection during active phase");

        // 阶段 B：长静默期（不再喂入）——回显逐级衰减耗尽 → 真空重启
        for i in 3000..10_000 {
            if let Some(e) = p.tick(None) {
                assert!(is_valid(e), "iter {i}: {e}");
            }
        }
        assert!(p.kicks > 0, "echo never exhausted into vacuum restart");
    }
}
