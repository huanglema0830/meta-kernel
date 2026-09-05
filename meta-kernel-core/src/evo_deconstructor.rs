//! # 进化解构器（Evo Deconstructor）
//!
//! 正源场域模型的解构引擎：把任何模式解构为可化合的**胶粒元素（层级 1）**。
//! 解构口径（Phase 4 §2.4）：
//!
//! - **时间三量**：从模式历史提取
//!   - 存量 stock —— 稳定部分（历史稳健均值）；
//!   - 变量 variable —— 波动部分（归一化标准差）；
//!   - 增量 delta —— 趋势方向（净变化的方向与幅度，∈[-1,1]）；
//! - **空间编码**：从模式元素按层级切分
//!   - 基础元素（层级 1-3）；编织原理（层级 4-7）；
//! - **输出**：层级 1 胶粒元素流（含时间三量编码），供化合（thinking_chain）使用。

use crate::math::clamp01;
use crate::ontology::{self, Element, Pattern};

/// 时间三量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeTriple {
    /// 存量（稳定部分，0-1）。
    pub stock: f32,
    /// 变量（波动部分，0-1）。
    pub variable: f32,
    /// 增量（趋势方向，-1..1；>0 上升趋势 / <0 下降 / ≈0 平稳）。
    pub delta: f32,
}

impl TimeTriple {
    /// 增量作为 0-1 强度（供胶粒编码）。
    pub fn delta_intensity(self) -> f32 {
        (self.delta + 1.0) / 2.0
    }
}

/// 提取时间三量（空历史 → 全 0 锚点）。
pub fn time_triple(history: &[f64]) -> TimeTriple {
    if history.is_empty() {
        return TimeTriple { stock: 0.0, variable: 0.0, delta: 0.0 };
    }
    let n = history.len() as f64;
    let mean = history.iter().sum::<f64>() / n;
    let var = history.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    let std = var.sqrt();
    let first = history.first().copied().unwrap_or(mean);
    let last = history.last().copied().unwrap_or(mean);
    let net = (last - first).clamp(-1.0, 1.0) as f32;
    TimeTriple {
        stock: clamp01(mean as f32),
        variable: clamp01((std / (1.0 + std)) as f32),
        delta: if net.abs() < 1e-4 { 0.0 } else { net },
    }
}

/// 空间编码：返回 (基础元素 1-3, 编织原理 4-7)。
pub fn space_encoding(elements: &[Element]) -> (Vec<Element>, Vec<Element>) {
    let basis: Vec<Element> = elements
        .iter()
        .filter(|e| (1..=3).contains(&e.level))
        .copied()
        .collect();
    let weave: Vec<Element> = elements
        .iter()
        .filter(|e| (4..=7).contains(&e.level))
        .copied()
        .collect();
    (basis, weave)
}

/// 解构到胶粒（层级 1）：基础元素 + 时间三量编码，全部 ≤ 层级 1。
pub fn deconstruct_to_granules(p: &Pattern) -> Vec<Element> {
    let mut out = Vec::new();
    // 空间：层级向下拆到 1
    if !p.elements.is_empty() {
        out.extend(ontology::decompose(p, 1));
    }
    // 时间：三量各编码为一条胶粒（强度即量值）
    let tt = time_triple(&p.history);
    out.push(Element::new(1, tt.stock as f64));
    out.push(Element::new(1, tt.variable as f64));
    out.push(Element::new(1, tt.delta_intensity() as f64));
    // 去零：胶粒 0 强度无化合价值（0 锚点保留意义除外）——保留强度>0 的
    out.retain(|e| e.intensity > 1e-6);
    if out.is_empty() {
        out.push(Element::new(1, 0.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_history_is_anchor_zero() {
        let tt = time_triple(&[]);
        assert_eq!(tt, TimeTriple { stock: 0.0, variable: 0.0, delta: 0.0 });
    }

    #[test]
    fn rising_ramp_extracts_positive_delta() {
        let hist: Vec<f64> = (0..10).map(|i| 0.1 + i as f64 * 0.04).collect(); // 0.10→0.46 单调升
        let tt = time_triple(&hist);
        assert!(tt.stock > 0.1 && tt.stock < 0.5, "stock={}", tt.stock);
        assert!(tt.delta > 0.0, "上升趋势 delta>0: {}", tt.delta);
        assert!(tt.variable >= 0.0 && tt.variable < 1.0);
        assert!((tt.delta_intensity() - (tt.delta + 1.0) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn constant_history_variable_zero() {
        let hist = vec![0.5; 24];
        let tt = time_triple(&hist);
        assert!((tt.stock - 0.5).abs() < 1e-3);
        assert!(tt.variable < 1e-3, "零波动: {}", tt.variable);
        assert_eq!(tt.delta, 0.0);
    }

    #[test]
    fn falling_ramp_negative_delta() {
        let hist: Vec<f64> = (0..8).map(|i| 0.8 - i as f64 * 0.09).collect();
        assert!(time_triple(&hist).delta < 0.0);
    }

    #[test]
    fn space_encoding_splits_by_level_bands() {
        let els = vec![
            Element::new(1, 0.4),
            Element::new(3, 0.5),
            Element::new(6, 0.6),
            Element::new(8, 0.9),
        ];
        let (basis, weave) = space_encoding(&els);
        assert_eq!(basis.len(), 2);
        assert_eq!(weave.len(), 1);
        assert!(basis.iter().all(|e| (1..=3).contains(&e.level)));
        assert!(weave.iter().all(|e| (4..=7).contains(&e.level)));
    }

    #[test]
    fn granules_are_level_one_and_usable() {
        let p = Pattern::new(vec![Element::new(6, 0.8), Element::new(2, 0.4)])
            .with_history(vec![0.1, 0.2, 0.3, 0.4]);
        let g = deconstruct_to_granules(&p);
        assert!(!g.is_empty());
        assert!(g.iter().all(|e| e.level <= 1), "胶粒必须 ≤ 层级1");
        // 时间三量已编码（应有强度≈0.25 的存量胶粒）
        assert!(g.iter().any(|e| (e.intensity - 0.25).abs() < 0.06));
    }
}
