//! # `Quad` 数据契约（四元组：紧张 / 平静 / 喜欢 / 安全）
//!
//! ## 为什么单独一个文件
//!
//! `l4_risk`（四条戒律判定）需要 `Quad` 作为**输入数据**，而 `Quad` 原本定义在 `l5_quad`
//! （该模块整体因 **alloc 在算法内部**留待 2.3）。为了不把整个 `l5_quad` 拖进来，
//! 这里抽出**只含纯数据与无 alloc 方法**的契约版本。
//!
//! ## ⚠️ 同步要求（**必须遵守**）
//!
//! 本结构与 `meta-kernel-core::l5_quad::Quad` 的**字段必须逐字一致**（顺序亦同）：
//!
//! | 字段 | 类型 | 含义 |
//! |---|---|---|
//! | `tension` | `f64` | 紧张 |
//! | `calm` | `f64` | 平静 |
//! | `liking` | `f64` | 喜欢 |
//! | `safety` | `f64` | 安全 |
//!
//! **留在 std 侧的**：`to_text` / `from_text`（序列化，需要 `String`/`Vec`）。
//! **改动纪律**：若 std 版增删字段或改顺序 ⇒ **必须同步改这里**；反之亦然。
//! 二者不一致时，**以 std 版（`meta-kernel-core`）为单一事实源**。

/// 四元组：紧张 / 平静 / 喜欢 / 安全（各 0..1）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    /// 紧张
    pub tension: f64,
    /// 平静
    pub calm: f64,
    /// 喜欢
    pub liking: f64,
    /// 安全
    pub safety: f64,
}

impl Default for Quad {
    /// 默认基线：平静略高、紧张低（"无事发生时"的稳态）。与 std 版一致。
    fn default() -> Self {
        Self { tension: 0.2, calm: 0.6, liking: 0.5, safety: 0.6 }
    }
}

impl Quad {
    /// 转数组（顺序：`tension, calm, liking, safety`）。
    #[must_use]
    pub fn to_array(&self) -> [f64; 4] {
        [self.tension, self.calm, self.liking, self.safety]
    }

    /// 由数组构造，值钳制到 `0..1`（与 std 版同口径）。
    #[must_use]
    pub fn from_array(a: [f64; 4]) -> Self {
        Self {
            tension: a[0].clamp(0.0, 1.0),
            calm: a[1].clamp(0.0, 1.0),
            liking: a[2].clamp(0.0, 1.0),
            safety: a[3].clamp(0.0, 1.0),
        }
    }

    /// 主导变量（值最高者）：返回 `(索引, 值)`。
    #[must_use]
    pub fn dominant(&self) -> (usize, f64) {
        let a = self.to_array();
        let mut bi = 0;
        for i in 1..4 {
            if a[i] > a[bi] {
                bi = i;
            }
        }
        (bi, a[bi])
    }

    /// 主导变量标签。
    #[must_use]
    pub fn dominant_label(&self) -> &'static str {
        ["紧张", "平静", "喜欢", "安全"][self.dominant().0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_core_baseline() {
        let q = Quad::default();
        assert_eq!(q.to_array(), [0.2, 0.6, 0.5, 0.6]);
    }

    #[test]
    fn from_array_clamps() {
        let q = Quad::from_array([-1.0, 2.0, 0.5, 0.5]);
        assert_eq!(q.to_array(), [0.0, 1.0, 0.5, 0.5]);
    }

    #[test]
    fn dominant_picks_max() {
        let q = Quad::from_array([0.1, 0.9, 0.5, 0.5]);
        assert_eq!(q.dominant().0, 1);
        assert_eq!(q.dominant_label(), "平静");
    }
}
