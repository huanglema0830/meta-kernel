//! # 负扰动过滤与安全阀（Sanitizer）
//!
//! Phase 2 全模块的"前置保险"（发起人指令：**所有负的都得让它消失**）。
//! 五大机制 + 守护进程：
//!
//! 1. **软钳位**：NPB 层强制将一切输入映射到 [0,1]；负值统一归零 `max(0, input)`；
//! 2. **负反馈雪崩保护**：斐波那契引擎参与运算前绝对值保护 `a=|a|, b=|b|`；
//! 3. **资源配额**：每个 Observer 分配内存/CPU 预算，超出即**强制休眠（非杀死）**；
//! 4. **合作奖励**：奖励 = 全局健康度 × 个体贡献（防止"杀它"行为）；
//! 5. **末那识监控器（守护进程）**：定期检查所有 Observer 健康度，异常者注入干扰种子；
//! 6. **负值自动湮灭**：任何计算输出若 `< 0` 直接置 0.0（最终保险）。
//!
//! 全部机制零依赖、纯函数优先、可单测。

/// 最终输出保险：一切计算输出经此函数落地（负值→0，超限→钳位）。
#[inline]
pub fn finalize(value: f32) -> f32 {
    if value < 0.0 {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}

/// 软钳位：NPB 层输入映射到 [0,1]（负值统一归零）。
#[inline]
pub fn soft_clamp(input: f32) -> f32 {
    input.max(0.0).min(1.0)
}

/// 负值归零（语义与 soft_clamp 下界一致，独立命名便于调用点自文档化）。
#[inline]
pub fn negative_to_zero(value: f32) -> f32 {
    value.max(0.0)
}

/// 负反馈雪崩保护：`a,b` 绝对值化后返回（供斐波那契引擎调用）。
#[inline]
pub fn abs_guard(a: f32, b: f32) -> (f32, f32) {
    (a.abs(), b.abs())
}

/// 资源配额状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaStatus {
    /// 配额内，可继续运行。
    Active,
    /// 超出配额，强制休眠（非杀死）。
    Dormant,
}

/// Observer 资源配额：内存/CPU 预算 + 强制休眠框架。
///
/// 原则：**超限即休眠，绝不杀死**——休眠可在下轮恢复并接受重编（正源系统接管）。
#[derive(Debug, Clone)]
pub struct ObserverQuota {
    /// Observer 编号。
    pub id: u64,
    /// 已用 CPU tick。
    used_ticks: u64,
    /// 已用内存字节。
    used_bytes: u64,
    /// CPU 预算（tick）。
    cpu_budget: u64,
    /// 内存预算（字节）。
    mem_budget: u64,
    /// 休眠剩余 tick（>0 表示处于强制休眠）。
    dormant_ticks: u64,
}

impl ObserverQuota {
    /// 新建配额。
    pub const fn new(id: u64, cpu_budget: u64, mem_budget: u64) -> Self {
        Self { id, used_ticks: 0, used_bytes: 0, cpu_budget, mem_budget, dormant_ticks: 0 }
    }

    /// 消耗一次资源用量；若已休眠则忽略（休眠者不消耗）。
    pub fn charge(&mut self, ticks: u64, bytes: u64) -> QuotaStatus {
        if self.dormant_ticks > 0 {
            self.dormant_ticks -= 1;
            return QuotaStatus::Dormant;
        }
        self.used_ticks = self.used_ticks.saturating_add(ticks);
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        if self.used_ticks > self.cpu_budget || self.used_bytes > self.mem_budget {
            // 超出预算 → 强制休眠（非杀死）
            self.dormant_ticks = self.cpu_budget / 4 + 1;
            QuotaStatus::Dormant
        } else {
            QuotaStatus::Active
        }
    }

    /// 当前是否处于休眠。
    pub const fn is_dormant(&self) -> bool {
        self.dormant_ticks > 0
    }

    /// 手动释放休眠（如正源系统重编完成后唤醒）。
    pub fn wake(&mut self) {
        self.dormant_ticks = 0;
        self.used_ticks = 0;
        self.used_bytes = 0;
    }

    /// 资源占用率（0..=1，用于健康度计算）。
    pub fn utilization(&self) -> f32 {
        let c = self.used_ticks as f32 / self.cpu_budget.max(1) as f32;
        let m = self.used_bytes as f32 / self.mem_budget.max(1) as f32;
        c.max(m).min(1.0)
    }
}

/// 合作奖励：`全局健康度 × 个体贡献`。
///
/// 设计意图：贡献被全局健康度缩放——个体"杀它/抢资源"会拉低全局健康度，
/// 从而同时降低自身奖励，从根本上抑制攻击行为。
#[inline]
pub fn cooperative_reward(global_health: f32, contribution: f32) -> f32 {
    if contribution < 0.0 {
        return 0.0; // 负贡献不产生奖励
    }
    finalize(global_health) * contribution
}

/// 末那识监控器（守护进程）：检查健康度，对异常 Observer 注入干扰种子。
///
/// 干扰种子用于"打乱异常行为"，随后通常交由正源系统重编；注入值经软钳位。
#[derive(Debug, Clone)]
pub struct ManasMonitor {
    /// 健康度告警阈值（低于即视为异常）。
    pub warn_threshold: f32,
}

impl Default for ManasMonitor {
    fn default() -> Self {
        Self { warn_threshold: 0.3 }
    }
}

impl ManasMonitor {
    /// 返回需要干扰的 Observer 编号列表（健康度 < 阈值）。
    pub fn scan(&self, health: &[f32]) -> Vec<u64> {
        health
            .iter()
            .enumerate()
            .filter(|(_, h)| **h < self.warn_threshold)
            .map(|(i, _)| i as u64)
            .collect()
    }

    /// 生成一根干扰种子（确定性伪随机，范围 [0,1]）。
    pub fn disturbance_seed(&self, salt: u64) -> f32 {
        let x = salt.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17);
        soft_clamp(((x >> 33) as u32 as f32) / (u32::MAX as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_annihilates_negatives() {
        assert_eq!(finalize(-0.5), 0.0);
        assert_eq!(finalize(-1e30), 0.0);
        assert_eq!(finalize(0.5), 0.5);
        assert_eq!(finalize(1.5), 1.0);
    }

    #[test]
    fn soft_clamp_maps_to_unit_interval() {
        assert_eq!(soft_clamp(-3.0), 0.0);
        assert_eq!(soft_clamp(0.25), 0.25);
        assert_eq!(soft_clamp(7.0), 1.0);
        assert_eq!(negative_to_zero(-0.1), 0.0);
        assert_eq!(negative_to_zero(0.1), 0.1);
    }

    #[test]
    fn abs_guard_protects_fib_from_snowball() {
        let (a, b) = abs_guard(-0.8, 0.9);
        assert_eq!(a, 0.8);
        assert_eq!(b, 0.9);
        let (a, b) = abs_guard(-0.1, -0.2);
        assert_eq!(a, 0.1);
        assert_eq!(b, 0.2);
    }

    #[test]
    fn quota_sleeps_instead_of_killing() {
        let mut q = ObserverQuota::new(1, 10, 1024);
        for _ in 0..9 {
            assert_eq!(q.charge(1, 0), QuotaStatus::Active);
        }
        // 第 10 tick 超预算 → 强制休眠而非杀死
        assert_eq!(q.charge(1, 0), QuotaStatus::Dormant);
        assert!(q.is_dormant());
        // 休眠期 charge 仍返回 Dormant（未被杀死，可被唤醒）
        let st = q.charge(0, 0);
        assert_eq!(st, QuotaStatus::Dormant);
        q.wake();
        assert!(!q.is_dormant());
    }

    #[test]
    fn cooperative_reward_discourages_attack() {
        // 健康全局 × 高贡献 → 高奖励
        assert!((cooperative_reward(1.0, 0.8) - 0.8).abs() < 1e-6);
        // 病态全局 × 高贡献 → 奖励被压低（抑制"我强就行"）
        assert!((cooperative_reward(0.2, 0.8) - 0.16).abs() < 1e-6);
        // 负贡献 → 零奖励（不奖赏破坏）
        assert_eq!(cooperative_reward(1.0, -0.5), 0.0);
    }

    #[test]
    fn monitor_flags_unhealthy_and_seeds_are_safe() {
        let m = ManasMonitor::default();
        let flagged = m.scan(&[0.9, 0.1, 0.5, 0.29]);
        assert_eq!(flagged, vec![1, 3]);
        for salt in 0..50u64 {
            let s = m.disturbance_seed(salt);
            assert!((0.0..=1.0).contains(&s), "seed {salt}: {s}");
        }
        // 不同 salt 扰动不同（确定性但不退化）
        assert_ne!(m.disturbance_seed(1), m.disturbance_seed(2));
    }
}
