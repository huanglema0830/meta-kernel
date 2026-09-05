//! # 双链诊断（Double Chain）
//!
//! 每个问题都长着两条链：
//!
//! - **问题形成过程（formation）**：问题如何一步步被"喂大"——记录形成路径上的
//!   各阶段值（0-1，可来自 0-10 标尺的层级化观测）；
//! - **解决过程（resolution）**：问题如何一步步收束——记录求解路径的各阶段值。
//!
//! 诊断口径（v1.0 工程口径，可校准）：
//! - 收敛间隙 `gap = |resolution.last − formation.last|`（目标 = 形成链终点）；
//! - `formation 为空` → 未解（无问题定义无从解决）；
//! - `gap ≤ 0.05` → 已解（Solved）；`gap ≤ 0.20` → 部分收敛（Partial）；
//! - 其余 → 未解（Unresolved）。

/// 诊断判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 问题已收敛解决。
    Solved,
    /// 部分收敛（接近但仍有余差）。
    Partial,
    /// 未解决（无定义或差距过大）。
    Unresolved,
}

/// 双链诊断报告。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diagnosis {
    /// 形成过程步数。
    pub formation_steps: usize,
    /// 解决过程步数。
    pub resolution_steps: usize,
    /// 收敛间隙（0-1）。
    pub gap: f32,
    /// 判定。
    pub verdict: Verdict,
}

impl Default for Diagnosis {
    fn default() -> Self {
        Self { formation_steps: 0, resolution_steps: 0, gap: 1.0, verdict: Verdict::Unresolved }
    }
}

/// 双链：形成链 + 解决链（窗口有界）。
#[derive(Debug, Clone)]
pub struct DoubleChain {
    formation: Vec<f32>,
    resolution: Vec<f32>,
    cap: usize,
}

impl Default for DoubleChain {
    fn default() -> Self {
        Self::with_cap(256)
    }
}

impl DoubleChain {
    /// 新建双链。
    pub fn new() -> Self {
        Self::default()
    }

    /// 以窗口容量新建。
    pub fn with_cap(cap: usize) -> Self {
        assert!(cap >= 1);
        Self { formation: Vec::with_capacity(cap), resolution: Vec::with_capacity(cap), cap }
    }

    /// 记录形成过程一步（问题被喂大的阶段值，0-1）。
    pub fn push_formation(&mut self, v: f32) {
        self.formation.push(crate::sanitizer::finalize(v));
        if self.formation.len() > self.cap {
            self.formation.remove(0);
        }
    }

    /// 记录解决过程一步（求解收束的阶段值，0-1）。
    pub fn push_resolution(&mut self, v: f32) {
        self.resolution.push(crate::sanitizer::finalize(v));
        if self.resolution.len() > self.cap {
            self.resolution.remove(0);
        }
    }

    /// 执行双链诊断。
    pub fn diagnose(&self) -> Diagnosis {
        let Some(target) = self.formation.last().copied() else {
            return Diagnosis::default();
        };
        let Some(final_v) = self.resolution.last().copied() else {
            return Diagnosis {
                formation_steps: self.formation.len(),
                resolution_steps: 0,
                gap: 1.0,
                verdict: Verdict::Unresolved,
            };
        };
        let gap = (final_v - target).abs();
        let verdict = if gap <= 0.05 {
            Verdict::Solved
        } else if gap <= 0.20 {
            Verdict::Partial
        } else {
            Verdict::Unresolved
        };
        Diagnosis {
            formation_steps: self.formation.len(),
            resolution_steps: self.resolution.len(),
            gap,
            verdict,
        }
    }

    /// 当前形成链步数。
    pub fn formation_steps(&self) -> usize {
        self.formation.len()
    }

    /// 当前解决链步数。
    pub fn resolution_steps(&self) -> usize {
        self.resolution.len()
    }

    /// 清空回 0 锚点。
    pub fn reset_to_anchor(&mut self) {
        self.formation.clear();
        self.resolution.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_formation_is_unresolved() {
        let dc = DoubleChain::new();
        let d = dc.diagnose();
        assert_eq!(d.verdict, Verdict::Unresolved);
        assert_eq!(d.formation_steps, 0);
    }

    #[test]
    fn converging_resolution_is_solved() {
        let mut dc = DoubleChain::new();
        for v in [0.1, 0.35, 0.6, 0.8] {
            dc.push_formation(v);
        }
        // 形成链终点 0.8；解决链收敛到 0.8
        for v in [0.0, 0.4, 0.6, 0.78, 0.8] {
            dc.push_resolution(v);
        }
        let d = dc.diagnose();
        assert_eq!(d.formation_steps, 4);
        assert_eq!(d.resolution_steps, 5);
        assert!(d.gap <= 0.05, "gap={}", d.gap);
        assert_eq!(d.verdict, Verdict::Solved);
    }

    #[test]
    fn diverging_resolution_is_unresolved() {
        let mut dc = DoubleChain::new();
        dc.push_formation(0.9);
        dc.push_resolution(0.1);
        let d = dc.diagnose();
        assert!(d.gap > 0.2);
        assert_eq!(d.verdict, Verdict::Unresolved);
    }

    #[test]
    fn window_caps_keep_old_entries_out() {
        let mut dc = DoubleChain::with_cap(4);
        for i in 0..10 {
            dc.push_formation(i as f32 / 10.0);
        }
        assert_eq!(dc.formation_steps(), 4);
    }
}
