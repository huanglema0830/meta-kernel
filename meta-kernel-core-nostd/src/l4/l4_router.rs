//! L4 判定路由（对外接口）
//!
//! - 与 L3 交互（对外接口）：接收**七维当前值 `&[f64]`** → `Result<[f64; 7], RejectReason>`
//!   （通过返回原数据；拒绝返回原因——被拒绝的数据**丢弃、不缓存**）。
//! - 与 L5 交互（对内接口）：通过后场域状态向量与变量一起传递 L5；接口格式由 L5 设计时
//!   确定——此处通过 `route_passed` 预留传递载体（本层不消费）。
//!
//! 无状态：不持有任何缓存/残留 → 重复调用结果恒定（纯）。
//!
//! ## ⚠️ 拆分说明（路径 A-2，`no_std` 侧）
//!
//! 原 std 版签名用 `Vec<f64>`（`Result<Vec<f64>, _>`、`L5Payload.state: Vec<f64>`、
//! `extra: Option<Vec<f64>>`）——**需要 `alloc`**。
//! 本侧改为**固定长数组 `[f64; 7]`**（长度本就恒为 7，语义等价、零分配）：
//!
//! | 原（std） | 本侧（`no_std`） |
//! |---|---|
//! | `route(&[f64]) -> Result<Vec<f64>, _>` | `route(&[f64]) -> Result<[f64; 7], _>` |
//! | `route_state(..) -> Result<Vec<f64>, _>` | `route_state(..) -> Result<[f64; 7], _>` |
//! | `L5Payload { state: Vec<f64>, extra: Option<Vec<f64>> }` | `L5Payload { state: [f64; 7] }`（`extra` 留 std） |
//! | `route_passed(&[f64])` | `route_passed(&FieldState)` |
//!
//! **判据算法一字未改**（只改接口载体类型）。

use super::dimension::FieldState;
use super::l4_gate::{self, Decision, RejectReason};

/// 对外主入口：接收 7 维当前值（`&[f64]`，长度须为 7），返回判定结果。
///
/// 内部以 `baseline()`（健康态各维 = 1.0）为存量基线。
/// 说明：基线由调用方经 `route_with_baseline` 指定；此接口使用默认健康基线。
pub fn route(current: &[f64]) -> Result<[f64; 7], RejectReason> {
    route_with_baseline(current, &FieldState::baseline())
}

/// 指定基线的路由（基线 = 存量，健康态完整状态）。
pub fn route_with_baseline(
    current: &[f64],
    baseline: &FieldState,
) -> Result<[f64; 7], RejectReason> {
    let state =
        FieldState::from_slice(current).ok_or(RejectReason::Malformed(7, current.len()))?;
    route_state(&state, baseline)
}

/// 由 `FieldState` 直接判定并路由（内部共用）。
pub fn route_state(state: &FieldState, baseline: &FieldState) -> Result<[f64; 7], RejectReason> {
    match l4_gate::check_state(state, baseline) {
        Decision::Pass => Ok(state.to_array()),
        Decision::UnseasonalMeal { high_count } => Err(RejectReason::UnseasonalMeal(high_count)),
        Decision::Ungrasping { low_count } => Err(RejectReason::Ungrasping(low_count)),
    }
}

/// L4 → L5 预留传递载体（通过后的场域状态；L5 接口格式待其设计，本层仅占位不消费）。
///
/// `no_std` 版只带**状态数组**；原版的 `extra: Option<Vec<f64>>` 需要 `alloc` ⇒ **留 std 侧**。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct L5Payload {
    /// 通过戒律的场域状态向量（七个维度）。
    pub state: [f64; 7],
}

/// 通过后构造 L5 传递载荷（不缓存、调用即出）。
#[must_use]
pub fn route_passed(state: &FieldState) -> L5Payload {
    L5Payload { state: state.to_array() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_ok_returns_same_vector() {
        let v = [1.0, 1.1, 0.9, 1.0, 0.7, 1.2, 1.0];
        assert_eq!(route(&v), Ok(v));
    }

    #[test]
    fn route_rejects_high_and_reports_reason() {
        let v = [2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0];
        assert_eq!(route(&v), Err(RejectReason::UnseasonalMeal(3)));
        let v2 = [0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0];
        assert_eq!(route(&v2), Err(RejectReason::Ungrasping(3)));
    }

    #[test]
    fn route_rejects_malformed_length() {
        assert_eq!(route(&[1.0, 2.0]), Err(RejectReason::Malformed(7, 2)));
    }

    #[test]
    fn rejected_data_not_cached_no_residue() {
        let bad = [3.0, 3.0, 3.0, 1.0, 1.0, 1.0, 1.0];
        // 连续拒绝多次，结果一致且不产生任何可观察残留
        for _ in 0..5 {
            assert_eq!(route(&bad), Err(RejectReason::UnseasonalMeal(3)));
        }
        // 之后正常数据仍独立判定
        let good = [1.0; 7];
        assert_eq!(route(&good), Ok(good));
    }

    #[test]
    fn custom_baseline_respected() {
        let base = FieldState::new(2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0);
        // 当前=4.0 → 相对 base 偏离 2.0（>1.618）三处以上 → 拒绝
        let v = [4.0, 4.0, 4.0, 2.0, 2.0, 2.0, 2.0];
        assert_eq!(route_with_baseline(&v, &base), Err(RejectReason::UnseasonalMeal(3)));
        // 当前=base → 全偏离 1.0 → 通过
        let ok = [2.0; 7];
        assert_eq!(route_with_baseline(&ok, &base), Ok(ok));
    }

    #[test]
    fn payload_carries_state_through() {
        let v = [1.0, 1.1, 1.2, 1.0, 1.0, 1.0, 1.0];
        let payload = route_passed(&FieldState::from_array(v));
        assert_eq!(payload.state, v);
    }
}
