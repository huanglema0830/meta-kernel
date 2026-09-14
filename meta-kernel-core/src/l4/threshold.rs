//! 黄金常数（L4 戒律阈值）· **已接入基因库基础公式层（v0.107）**。
//!
//! - 0.618 = 黄金分割比（low 阈值：偏离过低 → 不捉持）
//! - 1.618 = 1 / 0.618（high 阈值：偏离过高 → 不非时食）
//!
//! 接线方式：判据以**常量公式**登记在基因库 **基础公式（元素层）**，名为
//! [`NAME_HIGH`] / [`NAME_LOW`]；L4 判定经 [`from_library`] 读取。
//! **改基因库即改判据**（验收项）；基因库缺项时回退本文件内置常量（不破坏既有行为）。

use crate::gene_library::GeneLibrary;

/// 黄金分割比低阈值。
pub const GOLDEN_LOW: f64 = 0.618_033_988_749_894_9;

/// 高阈值 = 1 / 低阈值。
pub const GOLDEN_HIGH: f64 = 1.618_033_988_749_895;

/// 基础公式层中登记「高判据」的公式名。
pub const NAME_HIGH: &str = "l4.threshold.high";
/// 基础公式层中登记「低判据」的公式名。
pub const NAME_LOW: &str = "l4.threshold.low";

/// L4 判据组（可从基因库基础公式层读取）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    pub high: f64,
    pub low: f64,
}

impl Thresholds {
    /// 内置缺省判据（黄金常量）。
    pub fn defaults() -> Self {
        Self { high: GOLDEN_HIGH, low: GOLDEN_LOW }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Self::defaults()
    }
}

/// 把 L4 判据**登记进基因库基础公式层**（幂等 upsert；宿主启动时调用一次即可）。
/// 返回 `(high_id, low_id)`。
pub fn seed_into(lib: &mut GeneLibrary) -> (u32, u32) {
    let sig = [1.0f64; 7];
    let hi = lib.set_base_constant(NAME_HIGH, GOLDEN_HIGH, sig);
    let lo = lib.set_base_constant(NAME_LOW, GOLDEN_LOW, sig);
    (hi, lo)
}

/// **从基因库基础公式层读取判据**；缺项回退内置黄金常量。
pub fn from_library(lib: &GeneLibrary) -> Thresholds {
    Thresholds {
        high: lib.base_constant(NAME_HIGH).unwrap_or(GOLDEN_HIGH),
        low: lib.base_constant(NAME_LOW).unwrap_or(GOLDEN_LOW),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reciprocal_relationship() {
        assert!((GOLDEN_HIGH - 1.0 / GOLDEN_LOW).abs() < 1e-12, "1.618 = 1 / 0.618");
    }

    #[test]
    fn golden_ratio_identity() {
        // φ² = φ + 1 → (1.618)² ≈ 2.618 = 1.618 + 1
        let phi = GOLDEN_HIGH;
        assert!((phi * phi - (phi + 1.0)).abs() < 1e-9);
    }

    #[test]
    fn from_empty_library_falls_back_to_constants() {
        let lib = GeneLibrary::new();
        let th = from_library(&lib);
        assert_eq!(th, Thresholds::defaults(), "空基因库 → 回退内置判据");
    }

    #[test]
    fn seed_then_read_gives_defaults() {
        let mut lib = GeneLibrary::new();
        let (hi_id, lo_id) = seed_into(&mut lib);
        assert!(hi_id >= 1 && lo_id >= 1 && hi_id != lo_id);
        let th = from_library(&lib);
        assert_eq!(th.high, GOLDEN_HIGH);
        assert_eq!(th.low, GOLDEN_LOW);
        assert_eq!(lib.base.len(), 2, "两条基础公式入库");
    }

    #[test]
    fn seeding_is_idempotent() {
        let mut lib = GeneLibrary::new();
        seed_into(&mut lib);
        seed_into(&mut lib);
        assert_eq!(lib.base.len(), 2, "重复 seed 不新增（upsert）");
    }

    /// **验收：改基因库即改判据**。
    #[test]
    fn changing_library_changes_thresholds() {
        let mut lib = GeneLibrary::new();
        seed_into(&mut lib);
        assert_eq!(from_library(&lib).high, GOLDEN_HIGH);
        lib.set_base_constant(NAME_HIGH, 2.5, [1.0; 7]);
        let th = from_library(&lib);
        assert!((th.high - 2.5).abs() < 1e-12, "改基因库 → 判据改变");
        assert_eq!(th.low, GOLDEN_LOW, "未改的那条不受影响");
    }
}
