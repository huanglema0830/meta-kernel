//! 黄金常数（L4 戒律阈值）。
//!
//! - 0.618 = 黄金分割比（low 阈值：偏离过低 → 不捉持）
//! - 1.618 = 1 / 0.618（high 阈值：偏离过高 → 不非时食）

/// 黄金分割比低阈值。
pub const GOLDEN_LOW: f64 = 0.618_033_988_749_894_9;

/// 高阈值 = 1 / 低阈值。
pub const GOLDEN_HIGH: f64 = 1.618_033_988_749_895;

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
}
