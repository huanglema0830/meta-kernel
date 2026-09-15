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
//! | `Compound`（默认） | `I = clamp01(S×V×Δ·(1+γ·驻点) + 0.3×吸收能量)` | 三者**同时存在**才化合；吸收能量来自能量池 |
//! | `Linear`（可选） | `I = clamp01(0.34S + 0.33V + 0.33Δ)` | 线性叠加（原 Phase 3 公式，保留兼容） |
//!
//! **能量吸收（指令升级）**：化合产物随能量吸收率变化——
//! `product = clamp01(S·V·Δ + absorbed_energy × WEIGHT_ABSORBED)`；
//! `absorbed_energy` 必须来自内核能量池（`energy::EnergyPool::absorbed`），非模拟值。
//!
//! 痕迹整合（痕迹层）：**每次 step 都会产生痕迹并注入**——
//! ① 依据本轮波动/化合/流动判定痕迹类型（风火水地，见 `trace::decide_type`）；
//! ② 痕迹回注"余势"（trace_memory），为后续补充增量提供微弱记忆加成；
//! ③ 痕迹可被上游（self_recognizer）收集成习气。

use crate::math::clamp01;
use crate::sanitizer::soft_clamp;
use crate::trace::{decide_type, Trace, TraceType};
use std::collections::VecDeque;

/// 线性模式权重：存量。
pub const W_STOCK: f32 = 0.34;
/// 线性模式权重：变量。
pub const W_VARIABLE: f32 = 0.33;
/// 线性模式权重：补充增量。
pub const W_SUPPLEMENT: f32 = 0.33;

/// 波粒催化增益（4.2：驻点强度对化合产物的放大系数）。
pub const CATALYST_GAIN: f32 = 0.25;

/// 能量吸收率（absorbed_energy → 化合产物的加权系数）。
pub const WEIGHT_ABSORBED: f32 = 0.3;

/// 痕迹回注系数（余势注入补充增量的比例）。
pub const TRACE_INJECT: f32 = 0.05;
/// 痕迹记忆保留率（每次步进后旧痕迹余势的衰减）。
pub const TRACE_MEMORY_DECAY: f32 = 0.6;
/// 链内痕迹窗口。
pub const TRACE_CAP: usize = 256;

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

/// 思考链：连续推演的无环链式存储（窗口有界，链长单调）+ 痕迹余势。
#[derive(Debug, Clone)]
pub struct ThinkingChain {
    nodes: VecDeque<ChainNode>,
    cap: usize,
    seq: u64,
    mode: Blend,
    trace_log: VecDeque<Trace>,
    trace_mem: f32,
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
        Self {
            nodes: VecDeque::with_capacity(cap),
            cap,
            seq: 0,
            mode: Blend::Compound,
            trace_log: VecDeque::with_capacity(TRACE_CAP.min(1024)),
            trace_mem: 0.0,
        }
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

    /// 连续推演入口：自动以上一轮创新增量作为本轮存量（立即降维），
    /// 并生成痕迹（风火水地）→ 回注余势。无外部能量吸收（absorbed=0）。
    pub fn step(&mut self, variable: f32, supplement: f32) -> f32 {
        self.step_impl(variable, supplement, 0.0, 0.0)
    }

    /// 带能量吸收的连续推演：absorbed_energy 从内核能量池读取（非模拟）。
    pub fn step_with_energy(&mut self, variable: f32, supplement: f32, absorbed_energy: f32) -> f32 {
        self.step_impl(variable, supplement, 0.0, absorbed_energy)
    }

    /// 波粒催化推演（3.2/4.2）：化合产物的生成考虑干涉驻点的强度；
    /// 同样生成痕迹并回注。无外部能量吸收。
    ///
    /// 化合模式下：`I = clamp01(S·V·Δ·(1 + CATALYST_GAIN·驻点强度))`——
    /// 驻点（粒子）作为"催化位"，在其位置与强度上放大化合产物。
    pub fn step_catalyzed(&mut self, variable: f32, supplement: f32, particle_strength: f32) -> f32 {
        self.step_impl(variable, supplement, particle_strength, 0.0)
    }

    /// 催化 + 能量吸收版（Kernel 主用：粒子强度 × 能量池吸收）。
    pub fn step_catalyzed_with_energy(
        &mut self,
        variable: f32,
        supplement: f32,
        particle_strength: f32,
        absorbed_energy: f32,
    ) -> f32 {
        self.step_impl(variable, supplement, particle_strength, absorbed_energy)
    }

    /// 步进实现（痕迹产生 + 注入 + 能量吸收核心）。
    fn step_impl(&mut self, variable: f32, supplement: f32, particle_strength: f32, absorbed_energy: f32) -> f32 {
        let stock = self.nodes.back().map(|n| n.innovation).unwrap_or(0.0);
        let s = soft_clamp(stock);
        let v = soft_clamp(variable);
        let d0 = soft_clamp(supplement);
        // 余势注入：仅在有补充增量流入时，用痕迹记忆微加成（无流入不改纯净口径）
        let d = if d0 > 0.0 {
            clamp01(d0 + self.trace_mem * TRACE_INJECT)
        } else {
            d0
        };
        let a = soft_clamp(absorbed_energy);
        let gain = 1.0 + CATALYST_GAIN * soft_clamp(particle_strength);
        let innovation = match self.mode {
            // 化合：product = S·V·Δ·(催化增益) + 0.3·吸收能量
            Blend::Compound => clamp01(s * v * d * gain + WEIGHT_ABSORBED * a),
            // 线性（遗留兼容）：纯加权，忽略能量/催化
            Blend::Linear => clamp01(s * W_STOCK + v * W_VARIABLE + d * W_SUPPLEMENT),
        };
        self.seq += 1;
        let node = ChainNode { seq: self.seq, stock: s, variable: v, supplement: d, innovation };
        if self.nodes.len() == self.cap {
            self.nodes.pop_front();
        }
        self.nodes.push_back(node);

        // 痕迹生成：波动/化合/流动 → 风火水地；指纹含能量流模式（吸收能量桶）
        let volatility = (v - supplement).abs().min(1.0);
        let tt = decide_type(volatility, innovation, d);
        let fp = (self.seq as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ ((innovation * 1e4) as u64)
            ^ (tt.code() as u64).wrapping_mul(0x1000_0000_01B3)
            ^ (((a * 15.999).round() as u64).min(15) << 28);
        let intensity = (innovation * 0.8 + 0.1).clamp(0.05, 1.0);
        let trace = Trace { step: self.seq, intensity, trace_type: tt, fingerprint: fp, energy_flow: a };
        if self.trace_log.len() == TRACE_CAP {
            self.trace_log.pop_front();
        }
        self.trace_log.push_back(trace);

        // 余势记忆更新（痕迹回注）
        self.trace_mem = self.trace_mem * TRACE_MEMORY_DECAY + intensity * (1.0 - TRACE_MEMORY_DECAY);
        node.innovation
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

    /// 链内痕迹数（每次 step 一条）。
    pub fn trace_log_len(&self) -> usize {
        self.trace_log.len()
    }

    /// 最近一条痕迹（step 生成）。
    pub fn last_trace(&self) -> Option<Trace> {
        self.trace_log.back().copied()
    }

    /// 当前余势（痕迹记忆，0-1）。
    pub fn trace_memory(&self) -> f32 {
        self.trace_mem
    }

    /// 清空回 0 锚点（连痕迹余势一并归零）。
    pub fn reset_to_anchor(&mut self) {
        self.nodes.clear();
        self.trace_log.clear();
        self.trace_mem = 0.0;
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
    fn catalyzed_step_amplifies_on_particle_site() {
        // 先种入存量（prev innovation = 0.5）
        let mut base = ThinkingChain::new();
        base.push(0.5, 1.0, 1.0);
        let plain = base.step_catalyzed(0.8, 0.8, 0.0); // 无驻点

        let mut boosted = ThinkingChain::new();
        boosted.push(0.5, 1.0, 1.0);
        let strong = boosted.step_catalyzed(0.8, 0.8, 0.9); // 驻点强度 0.9

        assert!(strong > plain, "驻点催化应放大: {strong} vs {plain}");
        assert!((plain - 0.5 * 0.8 * 0.8).abs() < 1e-5);
        assert!((strong - plain * (1.0 + CATALYST_GAIN * 0.9)).abs() < 1e-5);

        // 任一因子为 0 仍产出 0（即使有驻点催化）
        let mut zero = ThinkingChain::new();
        zero.push(0.5, 1.0, 1.0);
        assert_eq!(zero.step_catalyzed(0.0, 0.8, 0.9), 0.0);
    }

    #[test]
    fn each_step_generates_trace_and_memory() {
        let mut c = ThinkingChain::new();
        assert_eq!(c.trace_log_len(), 0);
        c.step(0.6, 0.4);
        assert_eq!(c.trace_log_len(), 1, "step 必产生痕迹");
        let t = c.last_trace().expect("trace");
        assert!(t.intensity > 0.0 && t.intensity <= 1.0);
        assert_eq!(t.step, 1);
        c.step(0.6, 0.4);
        assert_eq!(c.trace_log_len(), 2);
        assert!(c.trace_memory() > 0.0, "余势记忆应累积");
        // 痕迹类型在 风火水地 之内
        let _ = [TraceType::Wind, TraceType::Fire, TraceType::Water, TraceType::Earth];
        assert!(t.fingerprint != 0);
    }

    #[test]
    fn absorbed_energy_from_pool_raises_product() {
        // 验收：化合产物随能量吸收率变化（absorbed 来自能量池，非模拟）
        use crate::energy::EnergyPool;
        let mut pool = EnergyPool::new();
        pool.absorb(1.0); // 真实能量池入流

        let mut c = ThinkingChain::new();
        c.push(0.5, 1.0, 1.0); // 种入存量（innovation = 0.5）
        let without = c.step_with_energy(0.2, 0.2, 0.0); // 无吸收
        let with = c.step_with_energy(0.2, 0.2, pool.absorbed()); // 吸收能量池

        assert!(without > 0.0 && without < 0.05, "基底产物: {without}");
        assert!(with > without + 0.2, "能量吸收应显著抬升产物: {with} vs {without}");
        assert!(with <= 1.0);
        // 痕迹记录本次能量流
        let t = c.last_trace().unwrap();
        assert!(t.energy_flow > 0.9, "痕迹携带能量流: {}", t.energy_flow);
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
