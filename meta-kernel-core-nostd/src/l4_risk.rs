//! L4 · **风险判断戒律**（不害 / 不妄动 / 可回退 / 自承）——挂接四元组状态。
//!
//! 依据：发起人「四元组内在变量框架落地」§二「L4戒律挂接：不害/不妄动/可回退/自承 → 挂接四元组状态」
//! 与 P2-7「风险判断戒律（L4）」。
//!
//! ## 四条戒律
//! | 戒律 | 含义 | 本模块如何判断 |
//! |---|---|---|
//! | **不害** | 不使任何一方受损 | 触及面检查（用户数据/他者内核/安全机制 → 违） |
//! | **不妄动** | 不在**内在状态不稳**时动手 | **挂接四元组**：紧张高＋平静低，或安全低 → 违（先稳后动） |
//! | **可回退** | 必须能回到原状 | 不可逆或未声明回滚 → 违 |
//! | **自承** | 自己的因自己了（不推给他者） | **挂接四元组**：安全低时**必须显式承认为己方原因**，否则违 |
//!
//! ## 与既有 L4/L7 的关系（**不重复、不覆盖**）
//! - 本模块**只判"该不该动"**（风险侧），不执行、不改分级；
//! - 与 `l4::l4_gate`（场域节律拒绝层）**并列**：前者管"场域是否过载"，本模块管"动作是否该做"；
//! - 与 `l7::grade`（T0–T3 分级）**互补**：分级管"谁来确认"，本模块管"内在状态是否许可"。
//!
//! **纯逻辑、零依赖、无副作用**。

use crate::quad::Quad;

/// 动作的触及面（与 L7 分级保持一致的词汇，避免自创）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    /// 不触及外部状态。
    Nothing,
    /// 自己的文件 / 自身进程。
    SelfOnly,
    /// 系统级（服务/配置）。
    System,
    /// 用户数据 / 他者内核 / 安全机制 —— **不害红线**。
    Others,
}

/// 待评估的动作（最小字段集，便于与 L7 的 ActionSpec 对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiskInput {
    pub name: &'static str,
    pub exposure: Exposure,
    /// 是否可逆。
    pub reversible: bool,
    /// 是否已声明回滚。
    pub has_rollback: bool,
    /// **自承**：是否明确承认"这是己方原因/己方责任"（不推给他者）。
    pub acknowledged_own_cause: bool,
}

/// 单条戒律的判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precept {
    NotHarm,
    NotReckless,
    Reversible,
    OwnCause,
}

impl Precept {
    pub fn label(&self) -> &'static str {
        match self {
            Precept::NotHarm => "不害",
            Precept::NotReckless => "不妄动",
            Precept::Reversible => "可回退",
            Precept::OwnCause => "自承",
        }
    }
    pub fn all() -> [Precept; 4] {
        [Precept::NotHarm, Precept::NotReckless, Precept::Reversible, Precept::OwnCause]
    }
}

/// 风险裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskVerdict {
    /// 四条皆过 → 可动。
    Cleared,
    /// 内在状态未稳（不妄动）→ **先稳后动**（不是永久拒绝）。
    SteadyFirst(Precept),
    /// 硬违（不害/可回退）→ **拒绝**。
    Refused(Precept),
}

/// 四元组不稳的判据（阈值写死、可解释）。
pub const TENSION_HIGH: f64 = 0.7;
pub const CALM_LOW: f64 = 0.35;
pub const SAFETY_LOW: f64 = 0.35;

/// **风险裁定**（四条戒律逐条判；顺序=优先级：不害 → 可回退 → 不妄动 → 自承）。
pub fn assess(a: &RiskInput, quad: Quad) -> RiskVerdict {
    // ① 不害：触及他者 → 直接拒绝（最高优先级）
    if matches!(a.exposure, Exposure::Others) {
        return RiskVerdict::Refused(Precept::NotHarm);
    }
    // ② 可回退：不可逆或未声明回滚 → 拒绝
    if !a.reversible || !a.has_rollback {
        return RiskVerdict::Refused(Precept::Reversible);
    }
    // ③ 不妄动：**挂接四元组** —— 又紧又不稳，或安全感不足 → 先稳后动
    if (quad.tension > TENSION_HIGH && quad.calm < CALM_LOW) || quad.safety < SAFETY_LOW {
        return RiskVerdict::SteadyFirst(Precept::NotReckless);
    }
    // ④ 自承：状态越不稳，越要求"自己的因自己了"（安全低时已在③拦下；此处守"紧张偏高"档）
    if quad.tension > TENSION_HIGH && !a.acknowledged_own_cause {
        return RiskVerdict::SteadyFirst(Precept::OwnCause);
    }
    RiskVerdict::Cleared
}

/// 是否放行（`Cleared` 才放行）。
pub fn cleared(a: &RiskInput, quad: Quad) -> bool {
    assess(a, quad) == RiskVerdict::Cleared
}

// ⚠️ 拆分说明（路径 A-2）：原 std 版的 `explain(v, a) -> String`（人读解释，供 L6 呈现）
// **已留在 `meta-kernel-core` 一侧** —— 它需要 `String` / `format!`（`alloc`）。
// 本层只保留**纯判据**（`assess` / `cleared` 与各 `Precept` 的 `label()`）。

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_action() -> RiskInput {
        RiskInput {
            name: "重读配置",
            exposure: Exposure::SelfOnly,
            reversible: true,
            has_rollback: true,
            acknowledged_own_cause: true,
        }
    }
    fn calm_quad() -> Quad {
        Quad::default()
    }

    #[test]
    fn calm_state_clears_safe_action() {
        assert_eq!(assess(&ok_action(), calm_quad()), RiskVerdict::Cleared);
        assert!(cleared(&ok_action(), calm_quad()));
    }

    /// **验收：不害（触及他者 → 拒绝，与四元组无关）**。
    #[test]
    fn not_harm_refuses_others_even_when_calm_and_authorised() {
        let mut a = ok_action();
        a.exposure = Exposure::Others;
        assert_eq!(assess(&a, calm_quad()), RiskVerdict::Refused(Precept::NotHarm));
        // 即便内在状态非常平静、自承完整，也不放行
        let serene = Quad::from_array([0.0, 1.0, 0.8, 1.0]);
        assert_eq!(assess(&a, serene), RiskVerdict::Refused(Precept::NotHarm));
    }

    /// **验收：可回退（不可逆 → 拒绝）**。
    #[test]
    fn reversible_precept_refuses_irreversible() {
        let mut a = ok_action();
        a.reversible = false;
        assert_eq!(assess(&a, calm_quad()), RiskVerdict::Refused(Precept::Reversible));
        let mut b = ok_action();
        b.has_rollback = false;
        assert_eq!(assess(&b, calm_quad()), RiskVerdict::Refused(Precept::Reversible));
    }

    /// **验收：不妄动挂接四元组**（紧张高＋平静低 → 先稳后动）。
    #[test]
    fn not_reckless_is_wired_to_quad_state() {
        let a = ok_action();
        // 又紧又不稳
        let agitated = Quad::from_array([0.9, 0.2, 0.5, 0.7]);
        assert_eq!(assess(&a, agitated), RiskVerdict::SteadyFirst(Precept::NotReckless));
        // 安全感不足
        let unsafe_q = Quad::from_array([0.3, 0.6, 0.5, 0.1]);
        assert_eq!(assess(&a, unsafe_q), RiskVerdict::SteadyFirst(Precept::NotReckless));
        // 同样动作，在平静状态下放行 → **状态本身就是判据的一部分**
        assert_eq!(assess(&a, calm_quad()), RiskVerdict::Cleared);
    }

    /// **验收：自承**（紧张偏高但状态可控时，必须自承才放行）。
    #[test]
    fn own_cause_precept_is_wired_to_quad_state() {
        let mut a = ok_action();
        a.acknowledged_own_cause = false;
        let tense_but_ok = Quad::from_array([0.8, 0.6, 0.5, 0.6]);
        assert_eq!(assess(&a, tense_but_ok), RiskVerdict::SteadyFirst(Precept::OwnCause));
        // 承认后放行
        a.acknowledged_own_cause = true;
        assert_eq!(assess(&a, tense_but_ok), RiskVerdict::Cleared);
        // 不紧张时，不要求自承（低张力场景不设门槛）
        let mut b = ok_action();
        b.acknowledged_own_cause = false;
        assert_eq!(assess(&b, Quad::from_array([0.3, 0.7, 0.5, 0.7])), RiskVerdict::Cleared);
    }

    #[test]
    fn priority_order_is_stable() {
        // 同时违多条 → 报最先命中者（不害 > 可回退 > 不妄动 > 自承）
        let a = RiskInput {
            name: "越界且不可逆",
            exposure: Exposure::Others,
            reversible: false,
            has_rollback: false,
            acknowledged_own_cause: false,
        };
        assert_eq!(assess(&a, Quad::from_array([0.9, 0.1, 0.1, 0.1])), RiskVerdict::Refused(Precept::NotHarm));
        let b = RiskInput { exposure: Exposure::System, ..a };
        assert_eq!(assess(&b, Quad::from_array([0.9, 0.1, 0.1, 0.1])), RiskVerdict::Refused(Precept::Reversible));
    }

    #[test]
    fn system_exposure_is_allowed_when_stable_and_reversible() {
        let a = RiskInput { exposure: Exposure::System, ..ok_action() };
        assert_eq!(assess(&a, calm_quad()), RiskVerdict::Cleared, "系统级本身不是红线");
    }

    #[test]
    fn labels_are_complete() {
        for p in Precept::all() {
            assert!(!p.label().is_empty());
        }
        assert_eq!(Precept::all().len(), 4);
        // ⚠️ 原 `explain()`（需要 String/format!）已**留 std 侧** ⇒ 其文本断言随之移出本层
        //    （由 `meta-kernel-core` 的同名测试覆盖，覆盖不丢）。
    }

    #[test]
    fn boundary_values_are_inclusive_as_documented() {
        let a = RiskInput { acknowledged_own_cause: false, ..ok_action() };
        // tension == TENSION_HIGH 不算"高"（严格大于）
        assert_eq!(assess(&a, Quad::from_array([TENSION_HIGH, 0.6, 0.5, 0.6])), RiskVerdict::Cleared);
        // safety == SAFETY_LOW 不算"低"
        assert_eq!(assess(&ok_action(), Quad::from_array([0.3, 0.6, 0.5, SAFETY_LOW])), RiskVerdict::Cleared);
    }
}
