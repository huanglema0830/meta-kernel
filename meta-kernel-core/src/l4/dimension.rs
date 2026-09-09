//! 七维场域状态向量定义与偏离计算（L4 · 判定原料）。
//!
//! `S = (t, f, a, φ, x, H, τ)`：时间/频率/幅度/相位/空间/熵/拓扑。

use crate::l4::threshold::GOLDEN_HIGH;

/// 七维场域状态向量（命名维度，全 f64）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldState {
    /// t 时间：速率/周期/时序。
    pub t: f64,
    /// f 频率：特征频率及分布。
    pub f: f64,
    /// a 幅度：强度/能量。
    pub a: f64,
    /// phi φ 相位：协同/同步度。
    pub phi: f64,
    /// x 空间：分布形态。
    pub x: f64,
    /// entropy H 熵：有序度/混沌度。
    pub entropy: f64,
    /// tau τ 拓扑：结构形态/连接。
    pub tau: f64,
}

impl FieldState {
    /// 七个维度的标准顺序（与 to_vec/from_vec 一致）。
    pub const ORDER: [&'static str; 7] = ["t", "f", "a", "phi", "x", "entropy", "tau"];

    pub fn new(t: f64, f: f64, a: f64, phi: f64, x: f64, entropy: f64, tau: f64) -> Self {
        Self { t, f, a, phi, x, entropy, tau }
    }

    /// 默认健康基线（各维 = 1.0，归一参考）。
    pub fn baseline() -> Self {
        Self::new(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0)
    }

    /// → Vec<f64>（对外接口载体）。
    pub fn to_vec(&self) -> Vec<f64> {
        vec![self.t, self.f, self.a, self.phi, self.x, self.entropy, self.tau]
    }

    /// ← Vec<f64>（长度必须为 7）。
    pub fn from_vec(v: &[f64]) -> Option<Self> {
        if v.len() != 7 {
            return None;
        }
        Some(Self::new(v[0], v[1], v[2], v[3], v[4], v[5], v[6]))
    }

    /// 逐维取索引（0..7 与 ORDER 对应）。
    pub fn get(&self, idx: usize) -> Option<f64> {
        match idx {
            0 => Some(self.t),
            1 => Some(self.f),
            2 => Some(self.a),
            3 => Some(self.phi),
            4 => Some(self.x),
            5 => Some(self.entropy),
            6 => Some(self.tau),
            _ => None,
        }
    }

    /// 与基线逐维偏离倍率 `|current / baseline|`。
    /// 除零守卫：基线 ≤ ε：当前 ≈0 → 1.0（无偏离）；否则 f64::MAX（无限偏离）。
    pub fn deviations(&self, base: &FieldState) -> [f64; 7] {
        let ratio = |cur: f64, bl: f64| -> f64 {
            if bl.abs() <= f64::EPSILON {
                if cur.abs() <= f64::EPSILON { 1.0 } else { f64::MAX }
            } else {
                (cur / bl).abs()
            }
        };
        [
            ratio(self.t, base.t),
            ratio(self.f, base.f),
            ratio(self.a, base.a),
            ratio(self.phi, base.phi),
            ratio(self.x, base.x),
            ratio(self.entropy, base.entropy),
            ratio(self.tau, base.tau),
        ]
    }

    /// 偏离 > 1.618 的维度数。
    pub fn count_high(&self, base: &FieldState) -> usize {
        self.deviations(base).iter().filter(|&&d| d > GOLDEN_HIGH).count()
    }

    /// 偏离 < 0.618 的维度数。
    pub fn count_low(&self, base: &FieldState) -> usize {
        let low = crate::l4::threshold::GOLDEN_LOW;
        self.deviations(base).iter().filter(|&&d| d < low).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l4::threshold::GOLDEN_LOW;

    #[test]
    fn struct_fields_roundtrip_vec() {
        let st = FieldState::new(0.5, 1.2, 3.0, 0.9, 2.2, 1.4, 0.8);
        let v = st.to_vec();
        assert_eq!(v.len(), 7);
        let back = FieldState::from_vec(&v).expect("7 元素可还原");
        assert_eq!(back, st);
        assert_eq!(back.get(0), Some(0.5));
        assert_eq!(back.get(6), Some(0.8));
        assert_eq!(back.get(7), None);
    }

    #[test]
    fn from_vec_rejects_wrong_length() {
        assert!(FieldState::from_vec(&[1.0, 2.0]).is_none());
        assert!(FieldState::from_vec(&[]).is_none());
        assert!(FieldState::from_vec(&[1.0; 8]).is_none());
    }

    #[test]
    fn deviations_match_manual_ratio() {
        let cur = FieldState::new(2.0, 0.5, 1.0, 1.618, 0.618, 3.0, 0.9);
        let base = FieldState::baseline();
        let d = cur.deviations(&base);
        assert!((d[0] - 2.0).abs() < 1e-9);
        assert!((d[1] - 0.5).abs() < 1e-9);
        assert!((d[3] - 1.618).abs() < 1e-9);
        assert!((d[4] - 0.618).abs() < 1e-9);
    }

    #[test]
    fn zero_baseline_guards() {
        let base = FieldState::new(0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        let cur0 = FieldState::new(0.0, 5.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        let d = cur0.deviations(&base);
        assert_eq!(d[0], 1.0, "0/0 → 无偏离");
        assert_eq!(d[1], f64::MAX, "非零/零基线 → 无限偏离");
    }

    #[test]
    fn counts_high_low_bounds() {
        let base = FieldState::baseline();
        let cur = FieldState::new(2.0, 2.0, 2.0, 1.0, 0.5, 0.5, 1.0);
        assert_eq!(cur.count_high(&base), 3, "high t/f/a");
        assert_eq!(cur.count_low(&base), 2, "low x/entropy");
        // 恰等于黄金常量阈值 → 不算越界（字面 0.618 < GOLDEN_LOW，须用精确常量）
        let edge = FieldState::new(GOLDEN_HIGH, GOLDEN_LOW, 1.0, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(edge.count_high(&base), 0, "恰等于阈值不算高");
        assert_eq!(edge.count_low(&base), 0, "恰等于阈值不算低");
    }
}
