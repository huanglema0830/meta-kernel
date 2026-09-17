//! L5 · 当前场 vs 本底场：亢/枯/平判定（l5_compare）。
//!
//! 逐分量偏离 = |当前 / 本底|：> GOLDEN_HIGH(1.618…) → 亢；< GOLDEN_LOW(0.618…) → 枯；
//! 否则 → 平。只读对比，不缓存（纯函数）。


//! 【2.3b 片5 迁移】与 `meta-kernel-core/src/l5_compare.rs` **同源**；仅作下述适配，其余**逐行逐字未改**：
//! ① 补 `alloc`/`core` 的 `use`（no_std 下 `Vec`/`vec!`/`Box`/`String`/`ToString`/`format!`/集合不在 prelude）
//! ② `std::` 路径 → `core::`/`alloc::`（同一类型或同一常量，**零语义变化**）
//! ③ 引入 `FloatOps` trait ⇒ 浮点方法在 no_std 下解析到 `fmath`（调用点一行未改）
//! ④ 本片清单**由 `coordination/tools/check_migration_closure.py` 的 `plan_core()` 脚本产出**（D40），**不手写**
#[allow(unused_imports)] // host(std) 下内在方法优先 ⇒ 本 import 可能"未使用"，这是 FloatOps 机制的必然结果
use crate::fmath::FloatOps;

use crate::l5_baseline::BaselineField;

/// 分量状态带。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// 亢：越节律上限。
    Kang,
    /// 枯：越节律下限。
    Ku,
    /// 平：节律内。
    Ping,
}

impl Band {
    pub fn name(&self) -> &'static str {
        match self {
            Band::Kang => "亢",
            Band::Ku => "枯",
            Band::Ping => "平",
        }
    }
    pub fn code(&self) -> &'static str {
        match self {
            Band::Kang => "Kang",
            Band::Ku => "Ku",
            Band::Ping => "Ping",
        }
    }
}

/// 逐分量对比 → 亢枯平（顺序 earth/water/fire/wind）。
pub fn compare(cur: &[f64; 4], base: &BaselineField) -> [Band; 4] {
    compare_with(cur, base, &crate::l4::threshold::Thresholds::defaults())
}

/// **判据可注入版**（v0.107 接线）：判据由基因库基础公式层提供
/// （经 [`crate::l4::threshold::from_library`]）。
pub fn compare_with(
    cur: &[f64; 4],
    base: &BaselineField,
    th: &crate::l4::threshold::Thresholds,
) -> [Band; 4] {
    let mut out = [Band::Ping; 4];
    for i in 0..4 {
        let bl = base.to_array()[i];
        let dev = if bl.abs() <= f64::EPSILON {
            if cur[i].abs() <= f64::EPSILON { 1.0 } else { f64::MAX }
        } else {
            (cur[i] / bl).abs()
        };
        out[i] = if dev > th.high {
            Band::Kang
        } else if dev < th.low {
            Band::Ku
        } else {
            Band::Ping
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> BaselineField {
        BaselineField { earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0, object: "o", established: "e" }
    }

    #[test]
    fn all_ping_when_equal_baseline() {
        let b = base();
        assert_eq!(compare(&[1.0; 4], &b), [Band::Ping; 4]);
    }

    #[test]
    fn fire_kang_water_ku() {
        let b = base();
        let cur = [1.0, 0.5, 2.0, 1.0];
        let p = compare(&cur, &b);
        assert_eq!(p[0], Band::Ping);
        assert_eq!(p[1], Band::Ku, "water 0.5 < 0.618 → 枯");
        assert_eq!(p[2], Band::Kang, "fire 2.0 > 1.618 → 亢");
        assert_eq!(p[3], Band::Ping);
    }

    #[test]
    fn boundary_equality_is_ping() {
        use crate::l4::threshold::{GOLDEN_HIGH as GH, GOLDEN_LOW as GL};
        let b = base();
        let cur = [GH, GL, 1.0, 1.0];
        let p = compare(&cur, &b);
        assert_eq!(p, [Band::Ping; 4], "恰等于阈值不算越界");
    }

    #[test]
    fn zero_baseline_guard() {
        let zb = BaselineField { earth: 0.0, water: 1.0, fire: 1.0, wind: 1.0, object: "o", established: "e" };
        let cur = [2.0, 1.0, 1.0, 1.0];
        assert_eq!(compare(&cur, &zb)[0], Band::Kang, "非零/零基线 → 亢");
    }

    #[test]
    fn pure_and_repeatable() {
        let b = base();
        let cur = [1.0, 0.5, 2.0, 3.0];
        let a = compare(&cur, &b);
        let c = compare(&cur, &b);
        assert_eq!(a, c);
        assert_eq!(a[3], Band::Kang);
    }
}
