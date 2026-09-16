//! 黄金常数（L4 戒律阈值）· **`no_std` 侧（纯数据）**
//!
//! - 0.618 = 黄金分割比（low 阈值：偏离过低 → 不捉持）
//! - 1.618 = 1 / 0.618（high 阈值：偏离过高 → 不非时食）
//!
//! ## ⚠️ 拆分说明（路径 A-2 · 阈值来源抽象）
//!
//! 原 std 版还有两个**依赖基因库**的函数，它们**留在了 `meta-kernel-core` 一侧**：
//!
//! | 原函数（std 侧保留） | 作用 | 为何不留本侧 |
//! |---|---|---|
//! | `seed_into(&mut GeneLibrary) -> (u32, u32)` | 把判据登记进基因库基础公式层 | 需要 `GeneLibrary`（含 `alloc`） |
//! | `from_library(&GeneLibrary) -> Thresholds` | 从基因库读判据（缺项回退常量） | 同上 |
//!
//! 于是本层**完全不出现 `GeneLibrary` 符号**：
//!
//! ```text
//! std 侧：   GeneLibrary ──from_library()──> Thresholds（纯数据）
//! no_std 侧：Thresholds ──> L4 判据（dimension / l4_gate / l4_router）
//! ```
//!
//! **判据算法一字未改**——只把"判据从哪来"这件事移出本层。
//! 后续阶段（2.4/2.5，裸机上）把 `Thresholds` 变成常量或自 ACPI 配置读入即可。

/// 黄金分割比低阈值。
pub const GOLDEN_LOW: f64 = 0.618_033_988_749_894_9;

/// 高阈值 = 1 / 低阈值。
pub const GOLDEN_HIGH: f64 = 1.618_033_988_749_895;

/// 基础公式层中登记「高判据」的公式名（**契约常量**：std 侧的 `seed_into`/`from_library` 用它）。
pub const NAME_HIGH: &str = "l4.threshold.high";
/// 基础公式层中登记「低判据」的公式名（同上）。
pub const NAME_LOW: &str = "l4.threshold.low";

/// L4 判据组（**纯数据**；由调用方注入，本层不关心其来源）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    /// 高阈值（偏离过高 → 不非时食）。
    pub high: f64,
    /// 低阈值（偏离过低 → 不捉持）。
    pub low: f64,
}

impl Thresholds {
    /// 内置缺省判据（黄金常量）。
    #[must_use]
    pub fn defaults() -> Self {
        Self { high: GOLDEN_HIGH, low: GOLDEN_LOW }
    }

    /// 显式构造（裸机侧从常量表 / ACPI 配置 / 内置表注入时用）。
    #[must_use]
    pub fn new(high: f64, low: f64) -> Self {
        Self { high, low }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Self::defaults()
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
    fn defaults_use_golden_constants() {
        let th = Thresholds::defaults();
        assert_eq!(th.high, GOLDEN_HIGH);
        assert_eq!(th.low, GOLDEN_LOW);
        assert_eq!(Thresholds::default(), th, "Default 与 defaults() 一致");
    }

    #[test]
    fn explicit_new_overrides() {
        let th = Thresholds::new(2.5, 0.4);
        assert_eq!(th.high, 2.5);
        assert_eq!(th.low, 0.4);
    }
}
