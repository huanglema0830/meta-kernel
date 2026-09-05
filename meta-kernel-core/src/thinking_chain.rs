//! # 思考链（Thinking Chain）
//!
//! 将发起人公式结构化为**连续推演**：
//!
//! ```text
//! 存量 S ＋ 变量 V ＋ 补充增量 Δ ＝ 创新增量 I
//! I 经采纳/验证 → 立即降维为新一轮存量（能量层级降维规则，见 ONTOLOGY_SPEC §6）
//! ```
//!
//! - 存量（stock）：上一轮沉淀下来的稳定成果（降维后的创新增量）；
//! - 变量（variable）：本轮现场扰动/种子（0 锚点机制下外部注入）；
//! - 补充增量（supplement）：正源库/镜像池供给的历史智慧；
//! - 创新增量（innovation）：三者按权重编织的新成果（∈[0,1]，饱和运算）。
//!
//! 工程口径（v1.0）：`I = clamp01(0.34·S + 0.33·V + 0.33·Δ)`，
//! 即任一输入可主导、整体归一（后续可按运行数据微调权重，策略同 MATH_SPEC A4/A5）。

use crate::math::clamp01;
use crate::sanitizer::soft_clamp;
use std::collections::VecDeque;

/// 权重：存量。
pub const W_STOCK: f32 = 0.34;
/// 权重：变量。
pub const W_VARIABLE: f32 = 0.33;
/// 权重：补充增量。
pub const W_SUPPLEMENT: f32 = 0.33;

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
}

impl Default for ThinkingChain {
    fn default() -> Self {
        Self::with_cap(512)
    }
}

impl ThinkingChain {
    /// 新建思考链。
    pub fn new() -> Self {
        Self::default()
    }

    /// 以指定窗口容量新建。
    pub fn with_cap(cap: usize) -> Self {
        assert!(cap >= 1);
        Self { nodes: VecDeque::with_capacity(cap), cap, seq: 0 }
    }

    /// 推进一步：`I = clamp01(0.34S + 0.33V + 0.33Δ)`，存储节点并返回 I。
    pub fn push(&mut self, stock: f32, variable: f32, supplement: f32) -> ChainNode {
        let s = soft_clamp(stock);
        let v = soft_clamp(variable);
        let d = soft_clamp(supplement);
        let innovation = clamp01(s * W_STOCK + v * W_VARIABLE + d * W_SUPPLEMENT);
        self.seq += 1;
        let node = ChainNode { seq: self.seq, stock: s, variable: v, supplement: d, innovation };
        if self.nodes.len() == self.cap {
            self.nodes.pop_front();
        }
        self.nodes.push_back(node);
        node
    }

    /// 连续推演入口：自动以上一轮创新增量作为本轮存量（立即降维），
    /// 外部只需给"变量 + 补充增量"。
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
    fn equation_blends_weights() {
        let mut c = ThinkingChain::new();
        // 存量 0.5 + 变量 0.0 + 补充 0.0 → 0.5*0.34 = 0.17
        let n = c.push(0.5, 0.0, 0.0);
        assert!((n.innovation - 0.17).abs() < 1e-5);
        // 全 1 → 1.0
        let n = c.push(1.0, 1.0, 1.0);
        assert!((n.innovation - 1.0).abs() < 1e-5);
    }

    #[test]
    fn step_uses_previous_innovation_as_stock() {
        let mut c = ThinkingChain::new();
        // 真空第一推：stock=0
        let v1 = c.step(1.0, 0.0);
        assert!((v1 - 0.33).abs() < 1e-5, "0*0.34 + 1*0.33 = 0.33: {v1}");
        // 第二推：stock = v1（降维），变量 0，补充 0
        let v2 = c.step(0.0, 0.0);
        assert!((v2 - v1 * 0.34).abs() < 1e-5, "连续降维: {v2}");
        assert_eq!(c.length(), 2);
        assert_eq!(c.nodes(), 2);
    }

    #[test]
    fn window_caps_nodes_but_length_grows() {
        let mut c = ThinkingChain::with_cap(8);
        for i in 0..100u32 {
            c.step((i % 5) as f32 / 5.0, 0.2);
        }
        assert_eq!(c.nodes(), 8, "窗口封顶");
        assert_eq!(c.length(), 100, "链长单调不减");
        assert!(is_valid(c.innovation()));
    }

    #[test]
    fn reset_returns_to_anchor() {
        let mut c = ThinkingChain::new();
        c.step(1.0, 0.0);
        c.reset_to_anchor();
        assert_eq!(c.length(), 0);
        assert_eq!(c.nodes(), 0);
        assert_eq!(c.innovation(), 0.0);
    }
}
