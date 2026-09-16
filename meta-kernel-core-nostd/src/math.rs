//! 基础数学工具：模糊饱和运算与 0-1 区间约束（见 docs/MATH_SPEC.md §2/§3）。
//!
//! 全部运算以 **0 锚点** 为下界、**1.0 饱和**为上界，永不产生越界值。

/// 饱和加法：`a ⊕ b = min(1.0, a + b)`。
///
/// 输入域假设为 [0,1]（见白皮书假设 A1），但仍对负数做防御式归零。
#[inline]
pub fn sat_add(a: f32, b: f32) -> f32 {
    (a.max(0.0) + b.max(0.0)).min(1.0)
}

/// 强制归一化到 [0, 1]。
#[inline]
pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// 是否为"有限且落在 [0,1]"的合法内核值（测试断言用）。
#[inline]
pub fn is_valid(x: f32) -> bool {
    x.is_finite() && (0.0..=1.0).contains(&x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sat_add_basic() {
        assert_eq!(sat_add(0.2, 0.3), 0.5);
        assert_eq!(sat_add(0.7, 0.4), 1.0); // 饱和
        assert_eq!(sat_add(1.0, 1.0), 1.0);
        assert_eq!(sat_add(-1.0, 0.5), 0.5); // 负值防御
        assert_eq!(sat_add(0.0, 0.0), 0.0); // 0 锚点
    }

    #[test]
    fn clamp_and_valid() {
        assert_eq!(clamp01(-0.5), 0.0);
        assert_eq!(clamp01(1.5), 1.0);
        assert!(is_valid(0.0) && is_valid(1.0) && is_valid(0.333));
        assert!(!is_valid(f32::NAN) && !is_valid(1.1) && !is_valid(-0.01));
    }
}
