//! # 思考链（Thinking Chain）
//!
//! 将发起人公式结构化为**连续推演**：
//!
//! ```text
//! 存量 S ＋ 变量 V ＋ 补充增量 Δ ＝ 创新增量 I
//! I 经采纳/验证 → 立即降维为新一轮存量（能量层级降维规则，见 ONTOLOGY_SPEC §6）
//! ```
//!
//! Phase 4 升级（化学变化层）：化合公式由**线性叠加**升级为**非线性化合**，
//! 二选一（默认化合）：
//!
//! | 模式 | 公式 | 语义 |
//! |---|---|---|
//! | `Compound`（默认） | `I = clamp01(S × V × Δ)` | 三者**同时存在**才化合；任一为 0 产物为 0 |
//! | `Linear`（可选） | `I = clamp01(0.34S + 0.33V + 0.33Δ)` | 线性叠加（原 Phase 3 公式，保留兼容） |

use crate::math::clamp01;
use crate::sanitizer::soft_clamp;
use std::collections::VecDeque;

/// 线性模式权重：存量。
pub const W_STOCK: f32 = 0.34;
/// 线性模式权重：变量。
pub const W_VARIABLE: f32 = 0.33;
/// 线性模式权重：补充增量。
pub const W_SUPPLEMENT: f32 = 0.33;

/// 化合模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    /// 非线性化合：`S×V×Δ`（默认）。
    Compound,
    /// 线性叠加：`0.34S+0.33V+0.33Δ`（可选兼容）。
    Linear,
}

impl Default for Blend {
    fn default() -> Self {
        Blend::Compound
    }
}

/// 单步推演节点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainNode {
    /// 全局步序号（从 1 起）。
    pub seq: u64,
    /// 存量（上轮创新增量降维而来）。
    pub stock: f32,
    /// 变量（本轮扰动）。
    pub variable: f32,
    /// 补充增量（历史智慧）。
    pub supplement: f32,
    /// 本轮创新增量（∈[0,1]）。
    pub innovation: f32,
}

/// 思考链：连续推演的无环链式存储（窗口有界，链长单调）。
#[derive(Debug, Clone)]
pub struct ThinkingChain {
    nodes: VecDeque<ChainNode>,
    cap: usize,
    seq: u64,
    mode: Blend,
}

impl Default for ThinkingChain {
    fn default() -> Self {
        Self::with_cap(512)
    }
}

impl ThinkingChain {
    /// 新建思考链（默认化合模式）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 以指定窗口容量新建。
    pub fn with_cap(cap: usize) -> Self {
        assert!(cap >= 1);
        Self { nodes: VecDeque::with_capacity(cap), cap, seq: 0, mode: Blend::Compound }
    }

    /// 切换化合模式（默认 `Compound`；需要旧线性行为时显式调用）。
    pub fn with_mode(mut self, mode: Blend) -> Self {
        self.mode = mode;
        self
    }

    /// 当前化合模式。
    pub const fn mode(&self) -> Blend {
        self.mode
    }

    /// 推进一步，存储节点并返回 I。
    ///
    /// - `Compound`：`I = clamp01(S·V·Δ)`（任一为 0 → 产物为 0）；
    /// - `Linear`：`I = clamp01(0.34S + 0.33V + 0.33Δ)`。
    pub fn push(&mut self, stock: f32, variable: f32, supplement: f32) -> ChainNode {
        let s = soft_clamp(stock);
        let v = soft_clamp(variable);
        let d = soft_clamp(supplement);
        let innovation = match self.mode {
            Blend::Compound => clamp01(s * v * d),
            Blend::Linear => clamp01(s * W_STOCK + v * W_VARIABLE + d * W_SUPPLEMENT),
        };
        self.seq += 1;
        let node = ChainNode { seq: self.seq, stock: s, variable: v, supplement: d, innovation };
        if self.nodes.len() == self.cap {
            self.nodes.pop_front();
        }
        self.nodes.push_back(node);
        node
    }

    /// 连续推演入口：自动以上一轮创新增量作为本轮存量（立即降维）。
    pub fn step(&mut self, variable: f32, supplement: f32) -> f32 {
        let stock = self.nodes.back().map(|n| n.innovation).unwrap_or(0.0);
        self.push(stock, variable, supplement).innovation
    }

    /// 当前最新节点（真空态 None）。
    pub fn latest(&self) -> Option<&ChainNode> {
        self.nodes.back()
    }

    /// 思考链长度：累计推演步数（单调不减）。
    pub fn length(&self) -> u64 {
        self.seq
    }

    /// 推演节点数：窗口内保留的节点个数。
    pub fn nodes(&self) -> usize {
        self.nodes.len()
    }

    /// 当前最新创新增量（真空态 0）。
    pub fn innovation(&self) -> f32 {
        self.nodes.back().map(|n| n.innovation).unwrap_or(0.0)
    }

    /// 清空回 0 锚点。
    pub fn reset_to_anchor(&mut self) {
        self.nodes.clear();
        self.seq = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::is_valid;

    #[test]
    fn compound_requires_all_three_factors() {
        let mut c = ThinkingChain::new(); // 默认化合
        assert_eq!(c.push(0.5, 0.0, 0.5).innovation, 0.0);
        assert_eq!(c.push(0.0, 0.5, 0.5).innovation, 0.0);
        assert_eq!(c.push(0.5, 0.5, 0.0).innovation, 0.0);
        assert_eq!(c.push(0.0, 0.0, 0.0).innovation, 0.0);
    }

    #[test]
    fn compound_multiplies_when_all_present() {
        let mut c = ThinkingChain::new();
        let n = c.push(0.5, 0.4, 0.2);
        assert!((n.innovation - 0.5 * 0.4 * 0.2).abs() < 1e-6, "{}", n.innovation);
        let n = c.push(1.0, 1.0, 1.0);
        assert!((n.innovation - 1.0).abs() < 1e-6);
    }

    #[test]
    fn linear_mode_preserves_legacy_formula() {
        let mut c = ThinkingChain::new().with_mode(Blend::Linear);
        let n = c.push(0.5, 0.0, 0.0);
        assert!((n.innovation - 0.17).abs() < 1e-5, "{}", n.innovation);
        let n = c.push(1.0, 1.0, 1.0);
        assert!((n.innovation - 1.0).abs() < 1e-5);
    }

    #[test]
    fn step_uses_previous_innovation_as_stock() {
        let mut c = ThinkingChain::new().with_mode(Blend::Linear);
        let v1 = c.step(1.0, 0.0);
        assert!((v1 - 0.33).abs() < 1e-5);
        let v2 = c.step(0.0, 0.0);
        assert!((v2 - v1 * 0.34).abs() < 1e-5);
        assert_eq!(c.length(), 2);
        assert_eq!(c.nodes(), 2);
    }

    #[test]
    fn window_caps_nodes_but_length_grows() {
        let mut c = ThinkingChain::with_cap(8);
        for i in 0..100u32 {
            c.push(0.5, (i % 5) as f32 / 5.0, 0.5);
        }
        assert_eq!(c.nodes(), 8);
        assert_eq!(c.length(), 100);
        assert!(is_valid(c.innovation()));
    }

    #[test]
    fn reset_returns_to_anchor() {
        let mut c = ThinkingChain::new();
        c.push(0.5, 0.5, 0.5);
        c.reset_to_anchor();
        assert_eq!(c.length(), 0);
        assert_eq!(c.nodes(), 0);
        assert_eq!(c.innovation(), 0.0);
    }
}
