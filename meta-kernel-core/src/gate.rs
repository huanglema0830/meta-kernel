//! # 闸门层（Gate）— 摩尼宝珠第二层：进化模式验证与黄金拆解
//!
//! 对输入模式做"进化模式"验证（三量完整：存量 + 变量 + 补充增量）；
//! 不符合的按 **×0.618**（黄金分割倒数）逐层拆解，直到通过或归为层级 1 胶粒。
//!
//! 五戒落点：
//! - **不杀生**：负值元素 → `Rejected`（不产出任何下游活动）；核心模块 → 禁止拆解（直接 Pass）。
//! - **不偷盗**：非纠缠/不完整输入不纳入正源，只拆解为胶粒原料（由调用方回收，不占有）。
//! - **不饮酒**：本模块只使用加法与 ×0.618 拆解，无其他算法。

use crate::ontology::{Element, Pattern};

/// 黄金拆解比（×0.618…；f64 精度版本）。
pub const DECOMPOSE_RATIO: f64 = 0.618_033_988_749_894_9;
/// 拆解到胶粒的最大层数（×0.618^16 ≈ 4.7e-4，已足够接近"微尘"）。
pub const MAX_DEPTH: u8 = 16;
/// 拆解单层强度上限（钳制，防累积越过 1）。
const ELEMENT_CAP: f64 = 0.9999;
/// 存量阈值：存在元素强度 > 0.1 视为有"存量"（三量之一）。
pub const STOCK_THRESHOLD: f64 = 0.1;

/// 闸门上下文：把"三量/核心"从模式内字段抽象为调用方运行时判定。
///
/// 仓库中 Pattern 无 metadata 字段——补充增量（心流凿空孪生命中 / 思考链存量注入）
/// 是主循环里的运行时量，故由调用方装配；核心（正源库已采纳/催化剂）同理。
#[derive(Debug, Clone, Copy, Default)]
pub struct GateCtx {
    /// 变量：演化观测充分（主循环 = 熵窗口 ≥ 3；独立使用亦可由 history 长度判定）。
    pub has_variable: bool,
    /// 补充增量：本 tick 有心流凿空孪生命中（或思考链存量注入兜底）。
    pub has_supplement: bool,
    /// 核心模块：不允许被拆解（正源库已采纳/催化剂；五戒·不杀生的豁免面）。
    pub is_core: bool,
}

/// 闸门判定结果。
#[derive(Debug, Clone, PartialEq)]
pub enum GateResult {
    /// 通过，返回精化后的模式。
    Pass(Pattern),
    /// 拆解到胶粒，作为原料回收（层 1，强度 ×0.618^16）。
    RecycledToGranules(Vec<Element>),
    /// 彻底拒绝（负值或完全无结构）。
    Rejected,
}

/// 闸门：进化模式验证 + 黄金逐层拆解。
#[derive(Debug, Clone)]
pub struct Gate {
    /// 最大拆解层数。
    pub max_depth: u8,
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    /// 默认闸门（最大拆解 16 层）。
    pub fn new() -> Self {
        Self { max_depth: MAX_DEPTH }
    }

    /// 带参构造（max_depth ≥ 1）。
    pub fn with_max_depth(max_depth: u8) -> Self {
        Self { max_depth: max_depth.max(1) }
    }

    /// 验证输入模式：
    /// 1. 负值拦截 → `Rejected`（不杀生）；
    /// 2. 核心或三量完整（存量+变量+补充增量）→ `Pass`；
    /// 3. 逐层 ×0.618 拆解，任一层三量完整 → `Pass`；
    /// 4. 拆到底仍不符合 → `RecycledToGranules`（胶粒原料）。
    pub fn check(&self, pattern: &Pattern, ctx: &GateCtx) -> GateResult {
        // 1. 负值拦截：任何负强度直接拒绝（不杀生）
        if pattern.elements.iter().any(|e| e.intensity < 0.0) {
            return GateResult::Rejected;
        }
        // 2. 核心模块禁止拆解（不杀生豁免面）；三量完整 → Pass
        if ctx.is_core || self.is_evolutionary(pattern, ctx) {
            return GateResult::Pass(pattern.clone());
        }
        // 3. 逐层黄金拆解
        let mut current = pattern.clone();
        for _ in 0..self.max_depth {
            current = self.decompose_one_layer(&current);
            if self.is_evolutionary(&current, ctx) {
                return GateResult::Pass(current);
            }
        }
        // 4. 拆到底仍不符合 → 胶粒原料
        let granules = self.decompose_to_granules(pattern);
        GateResult::RecycledToGranules(granules)
    }

    /// 进化模式判定：三量完整（存量 + 变量 + 补充增量）且非核心。
    pub fn is_evolutionary(&self, pattern: &Pattern, ctx: &GateCtx) -> bool {
        let has_stock = pattern.elements.iter().any(|e| e.intensity > STOCK_THRESHOLD);
        let has_variable = ctx.has_variable || pattern.history.len() > 2;
        let has_supplement = ctx.has_supplement;
        has_stock && has_variable && has_supplement
    }

    /// 拆解一层：每个元素强度 ×0.618，钳制 [0, 0.9999]（层级保持不变）。
    pub fn decompose_one_layer(&self, pattern: &Pattern) -> Pattern {
        let new_elements: Vec<Element> = pattern
            .elements
            .iter()
            .map(|e| Element {
                level: e.level,
                intensity: (e.intensity * DECOMPOSE_RATIO).clamp(0.0, ELEMENT_CAP),
            })
            .collect();
        Pattern {
            elements: new_elements,
            history: pattern.history.clone(),
        }
    }

    /// 拆解到胶粒状态：层级=1，强度=原强度 ×0.618^max_depth（钳 [0,1]）。
    pub fn decompose_to_granules(&self, pattern: &Pattern) -> Vec<Element> {
        let shrink = DECOMPOSE_RATIO.powi(self.max_depth as i32);
        pattern
            .elements
            .iter()
            .map(|e| Element {
                level: 1,
                intensity: (e.intensity * shrink).clamp(0.0, 1.0),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(level: u8, intensity: f64) -> Element {
        Element::new(level, intensity)
    }

    fn full_ctx() -> GateCtx {
        GateCtx { has_variable: true, has_supplement: true, is_core: false }
    }

    #[test]
    fn complete_pattern_passes() {
        let g = Gate::new();
        let p = Pattern {
            elements: vec![el(2, 0.5), el(5, 0.4)],
            history: vec![0.1, 0.2, 0.3, 0.4],
        };
        match g.check(&p, &full_ctx()) {
            GateResult::Pass(out) => {
                assert_eq!(out.elements.len(), 2);
                assert_eq!(out.elements[0].intensity, 0.5); // 原样返回，不加包装
            }
            other => panic!("完整模式应 Pass，得 {other:?}"),
        }
    }

    #[test]
    fn core_pattern_skips_decomposition() {
        let g = Gate::new();
        // 核心豁免：即使三量缺失（无 history/补充增量）也直接 Pass，禁止拆解
        let p = Pattern { elements: vec![el(3, 0.7)], history: Vec::new() };
        let ctx = GateCtx { is_core: true, ..GateCtx::default() };
        assert!(matches!(g.check(&p, &ctx), GateResult::Pass(_)), "核心模块不得被拆解");
    }

    #[test]
    fn negative_intensity_rejected() {
        let g = Gate::new();
        let p = Pattern { elements: vec![el(1, -0.2), el(3, 0.5)], history: Vec::new() };
        assert_eq!(g.check(&p, &full_ctx()), GateResult::Rejected, "负值 → 不杀生");
    }

    #[test]
    fn incomplete_pattern_recycled_to_granules() {
        let g = Gate::new();
        // 存量不足（无 >0.1 元素）→ 三量不完整 → 拆到底 → 胶粒
        let p = Pattern { elements: vec![el(4, 0.05)], history: vec![0.1, 0.1, 0.1] };
        match g.check(&p, &GateCtx { has_variable: true, has_supplement: true, is_core: false }) {
            GateResult::RecycledToGranules(gs) => {
                assert!(!gs.is_empty());
                assert!(gs.iter().all(|e| e.level == 1), "胶粒必须为层级 1");
                let expect = 0.05 * DECOMPOSE_RATIO.powi(MAX_DEPTH as i32);
                assert!((gs[0].intensity - expect).abs() < 1e-9, "{:?} != {}", gs[0].intensity, expect);
            }
            other => panic!("应回收为胶粒，得 {other:?}"),
        }
    }

    #[test]
    fn decompose_follows_golden_ratio_per_layer() {
        let g = Gate::new();
        let p = Pattern { elements: vec![el(2, 0.5)], history: Vec::new() };
        let one = g.decompose_one_layer(&p);
        assert!((one.elements[0].intensity - 0.5 * DECOMPOSE_RATIO).abs() < 1e-12);
        let g16 = g.decompose_to_granules(&p);
        let expect = 0.5 * DECOMPOSE_RATIO.powi(16);
        assert!((g16[0].intensity - expect).abs() < 1e-9, "×0.618^16 微尘");
    }

    #[test]
    fn evolutionary_needs_all_three_quantities() {
        let g = Gate::new();
        let p = Pattern { elements: vec![el(2, 0.5)], history: vec![0.1, 0.1, 0.1] };
        assert!(!g.is_evolutionary(&p, &GateCtx { has_variable: true, has_supplement: false, is_core: false }));
        assert!(g.is_evolutionary(&p, &GateCtx { has_variable: true, has_supplement: true, is_core: false }));
    }
}
