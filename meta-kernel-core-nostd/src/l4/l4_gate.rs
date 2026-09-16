//! L4 戒律判定引擎（纯函数；无副作用）。
//!
//! 判定逻辑：
//! 1. 输入场域当前状态 `S` 与基线 `S_baseline`（各 7 维）；
//! 2. 逐维偏离倍率 = |当前 / 基线|；
//! 3. N_high = 偏离 > 1.618 的维数；N_low = 偏离 < 0.618 的维数；
//! 4. N_high ≥ 3 → **不非时食**（拒绝）；N_low ≥ 3 → **不捉持**（拒绝）；否则通过。
//!
//! 戒律 = 场域自身内在节律的边界；判定不依赖任何外部输入/状态（纯函数）。
//! 同时命中时优先报告「不非时食」（高偏离更显性，规则确定化）。

use super::dimension::FieldState;

/// 戒律判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 通过戒律检查（变量连同场域状态可进入 L5）。
    Pass,
    /// 不非时食：≥3 维偏离过高（>1.618）。
    UnseasonalMeal { high_count: usize },
    /// 不捉持：≥3 维偏离过低（<0.618）。
    Ungrasping { low_count: usize },
}

/// 拒绝原因（对外接口 Result 的 Err 载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// 不非时食：场域过量偏离，拒绝进入 L5。
    UnseasonalMeal(usize),
    /// 不捉持：场域枯弱偏离，拒绝进入 L5。
    Ungrasping(usize),
    /// 输入格式非法（期望维数, 实得维数）——接口层拒绝，不入判定。
    Malformed(usize, usize),
}

/// 戒律判定主函数（纯函数）。返回 `Decision`。
/// 判据取内置黄金常量；**判据可注入版见 [`check_state_with`]**。
pub fn check_state(current: &FieldState, baseline: &FieldState) -> Decision {
    check_state_with(current, baseline, &super::threshold::Thresholds::defaults())
}

/// 戒律判定（**判据可注入**）。
///
/// `no_std` 侧入口：判据由调用方以 [`super::threshold::Thresholds`]（**纯数据**）注入。
/// std 侧若要"从基因库取判据"，先经 `meta-kernel-core::l4::threshold::from_library(&lib)`
/// 得到 `Thresholds`，再调本函数 ⇒ **本层不出现 `GeneLibrary` 符号**（拆分口径）。
pub fn check_state_with(
    current: &FieldState,
    baseline: &FieldState,
    th: &super::threshold::Thresholds,
) -> Decision {
    let high = current.count_high_with(baseline, th.high);
    let low = current.count_low_with(baseline, th.low);
    if high >= 3 {
        Decision::UnseasonalMeal { high_count: high }
    } else if low >= 3 {
        Decision::Ungrasping { low_count: low }
    } else {
        Decision::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> FieldState {
        FieldState::baseline()
    }

    #[test]
    fn pass_when_within_rhythm() {
        let s = FieldState::new(1.0, 1.1, 0.9, 1.0, 0.7, 1.2, 1.0);
        assert_eq!(check_state(&s, &base()), Decision::Pass);
    }

    #[test]
    fn unseasonal_meal_at_exactly_3_high() {
        let s = FieldState::new(2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(check_state(&s, &base()), Decision::UnseasonalMeal { high_count: 3 });
    }

    #[test]
    fn unseasonal_meal_2_high_not_triggered() {
        let s = FieldState::new(2.0, 2.0, 0.5, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(check_state(&s, &base()), Decision::Pass);
    }

    #[test]
    fn ungrasping_at_exactly_3_low() {
        let s = FieldState::new(0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(check_state(&s, &base()), Decision::Ungrasping { low_count: 3 });
    }

    #[test]
    fn both_over_limit_prefers_high() {
        let s = FieldState::new(2.0, 2.0, 2.0, 0.5, 0.5, 0.5, 1.0);
        assert_eq!(check_state(&s, &base()), Decision::UnseasonalMeal { high_count: 3 });
    }

    #[test]
    fn five_high_count_reported() {
        let s = FieldState::new(3.0, 3.0, 3.0, 3.0, 3.0, 1.0, 1.0);
        assert_eq!(check_state(&s, &base()), Decision::UnseasonalMeal { high_count: 5 });
    }

    #[test]
    fn boundary_equality_is_pass() {
        let s = FieldState::new(1.618, 0.618, 1.0, 1.618, 0.618, 1.0, 1.0);
        assert_eq!(check_state(&s, &base()), Decision::Pass);
    }

    #[test]
    fn pure_function_no_side_effects() {
        let s = FieldState::new(2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);
        let b = base();
        let s_before = s;
        let b_before = b;
        for _ in 0..3 {
            assert_eq!(check_state(&s, &b), Decision::UnseasonalMeal { high_count: 3 });
        }
        assert_eq!(s, s_before);
        assert_eq!(b, b_before);
    }

    #[test]
    fn infinite_deviation_counts_as_high() {
        // 基线某维 0 而当前非 0 → MAX 偏离 → 计入 high（构造 3 处触发拒绝）
        let cur = FieldState::new(2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);
        let zero_base = FieldState::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(check_state(&cur, &zero_base), Decision::UnseasonalMeal { high_count: 3 });
        // 单维 MAX 不足 3 → 通过
        let cur1 = FieldState::new(2.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        let zb1 = FieldState::new(0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(check_state(&cur1, &zb1), Decision::Pass);
    }
}

/// v0.107 接线测试：L4 判据来自**基因库基础公式层**。
/// `no_std` 侧替代测试：**判据可注入**。
///
/// 原 std 侧的 `mod library_wiring`（验证"从**基因库**读判据"）**保留在 `meta-kernel-core`**——
/// 它需要 `GeneLibrary`（含 alloc），不属于本层。
/// 这里改为验证**本层真正负责的事**：判据由调用方以 `Thresholds` 注入，且注入后**判定确实随之改变**。
#[cfg(test)]
mod threshold_injection {
    use super::*;
    use crate::l4::threshold::Thresholds;

    #[test]
    fn injected_high_threshold_changes_decision() {
        let b = FieldState::baseline();
        let s = FieldState::new(2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(check_state(&s, &b), Decision::UnseasonalMeal { high_count: 3 });
        let th = Thresholds::new(2.5, 0.618_033_988_749_894_9);
        assert_eq!(check_state_with(&s, &b, &th), Decision::Pass, "抬高判据 → 通过");
    }

    #[test]
    fn injected_low_threshold_changes_decision() {
        let b = FieldState::baseline();
        let s = FieldState::new(0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(check_state(&s, &b), Decision::Ungrasping { low_count: 3 });
        let th = Thresholds::new(1.618_033_988_749_895, 0.4);
        assert_eq!(check_state_with(&s, &b, &th), Decision::Pass, "压低判据 → 通过");
    }
}
