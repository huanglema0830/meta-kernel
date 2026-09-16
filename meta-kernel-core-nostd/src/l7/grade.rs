//! L7 · 动作分级（T0–T3）与授权判定。
//!
//! 依据：`docs/L7_EXECUTION_DESIGN.md §2.2`。分级是 **L7 的安全地基**——
//! 任何"能对外产生写作用"的动作，先分级，再判授权；**不可回滚者一律降级为拒绝**。
//!
//! | 级别 | 含义 | 授权要求 |
//! |---|---|---|
//! | **T0** | 只读（不触及任何状态） | 无需确认 |
//! | **T1** | 低风险、可逆（本应用内） | 首次确认 + 之后记忆（可撤销） |
//! | **T2** | 有副作用、影响面较大（系统作用域 / 触及网络配置） | **每次**确认 |
//! | **T3** | 不可逆 / 越界（用户数据 / 他者内核 / 安全机制 / 无回滚） | **一律拒绝** |
//!
//! 纯逻辑（零依赖）：不执行任何动作，只**判定**该不该、需不需要确认。

//! 【2.3b 迁移说明】本源文件与 `meta-kernel-core/src/l7/grade.rs` **同源**，逐字一致，
//! 仅作两处适配（其余一字未改，便于日后 diff 核对）：
//! 1. 新增 `use alloc::vec::Vec;`（no_std 下 `Vec` 不在 prelude）——**alloc 属标准分发，不破 C1**（D34-b）；
//! 2. 文件头插入本段说明（不改变任何代码语义）。

use alloc::vec::Vec;

/// 动作触及面（分级关键维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touches {
    /// 不触及任何状态（纯读）。
    Nothing,
    /// 本应用自己的文件（exe / ui / 脚本 / 本应用数据）。
    OwnFiles,
    /// 网络配置（IP / DNS / 代理 / hosts / 防火墙 / 路由）——**发起人红线**。
    NetworkConfig,
    /// 用户数据。
    UserData,
    /// 他者内核（别的内核实例）。
    OtherKernel,
    /// 安全机制（鉴权 / 沙箱 / 加密等）。
    Security,
}

/// 作用域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// 只读。
    ReadOnly,
    /// 本应用自身。
    SelfApp,
    /// 系统级。
    System,
}

/// 动作规格（分级输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    pub id: u32,
    pub name: &'static str,
    pub scope: Scope,
    pub touches: Touches,
    /// 是否可逆（能回到执行前状态）。
    pub reversible: bool,
    /// 是否已声明**回滚动作**。
    pub has_rollback: bool,
}

impl ActionSpec {
    /// 便捷构造（可逆 + 已声明回滚）。
    pub fn new(id: u32, name: &'static str, scope: Scope, touches: Touches) -> Self {
        Self { id, name, scope, touches, reversible: true, has_rollback: true }
    }
}

/// 动作等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    /// 只读。
    T0Read,
    /// 低风险、可逆。
    T1LowRisk,
    /// 需逐次确认。
    T2Confirm,
    /// 拒绝。
    T3Refuse,
}

impl Grade {
    /// 编码（供账本记录）。
    pub fn code(&self) -> u8 {
        match self {
            Grade::T0Read => 0,
            Grade::T1LowRisk => 1,
            Grade::T2Confirm => 2,
            Grade::T3Refuse => 3,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Grade::T0Read => "T0 只读",
            Grade::T1LowRisk => "T1 低风险可逆",
            Grade::T2Confirm => "T2 逐次确认",
            Grade::T3Refuse => "T3 拒绝",
        }
    }
}

/// 分级主函数（**先红线，再可逆性，最后作用域**；顺序即优先级）。
pub fn grade_of(s: &ActionSpec) -> Grade {
    // ① 红线（触及面）——最高优先级
    match s.touches {
        Touches::UserData | Touches::OtherKernel | Touches::Security => return Grade::T3Refuse,
        _ => {}
    }
    // ② 只读 / 不触及 → T0
    if matches!(s.scope, Scope::ReadOnly) || matches!(s.touches, Touches::Nothing) {
        return Grade::T0Read;
    }
    // ③ 不可逆或未声明回滚 → T3（"无法回滚的一律拒绝"）
    if !s.reversible || !s.has_rollback {
        return Grade::T3Refuse;
    }
    // ④ 系统作用域 / 触及网络配置 → T2
    if matches!(s.scope, Scope::System) || matches!(s.touches, Touches::NetworkConfig) {
        return Grade::T2Confirm;
    }
    // ⑤ 本应用内、可逆、有回滚 → T1
    Grade::T1LowRisk
}

/// 授权账（复用 L6 授权链语义：追加式、**可撤销**）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grants {
    /// 已授权动作 id（追加式）。
    pub granted: Vec<u32>,
    /// 已撤销动作 id（撤销后回到"逐次确认"）。
    pub revoked: Vec<u32>,
}

impl Grants {
    pub fn new() -> Self {
        Self::default()
    }
    /// 追加一条授权。
    pub fn grant(&mut self, action_id: u32) {
        if !self.granted.contains(&action_id) {
            self.granted.push(action_id);
        }
    }
    /// 撤销一条授权。
    pub fn revoke(&mut self, action_id: u32) {
        if !self.revoked.contains(&action_id) {
            self.revoked.push(action_id);
        }
    }
    /// 是否处于"已授权且未撤销"。
    pub fn is_granted(&self, action_id: u32) -> bool {
        self.granted.contains(&action_id) && !self.revoked.contains(&action_id)
    }
}

/// 授权判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authorization {
    /// 放行（可执行）。
    Allow,
    /// 需用户确认（本次不放行）。
    NeedConfirm,
    /// 拒绝（永不执行）。
    Refuse,
}

/// 授权判定（分级 + 授权账 + 本次是否已确认）。
///
/// - T0 → `Allow`（只读不需确认）
/// - T1 → **本次已确认 或 已记忆授权且未撤销** → `Allow`；否则 `NeedConfirm`
///   （"首次授权后免确认"：首次走 `confirmed_now=true`，之后靠 `Grants` 记忆）
/// - T2 → 本次已确认 → `Allow`；否则 `NeedConfirm`（**每次都要确认**，不受记忆授权影响）
/// - T3 → `Refuse`（**与是否确认无关**）
pub fn authorize(s: &ActionSpec, g: &Grants, confirmed_now: bool) -> (Authorization, Grade) {
    let grade = grade_of(s);
    let a = match grade {
        Grade::T0Read => Authorization::Allow,
        Grade::T1LowRisk => {
            if g.is_granted(s.id) || confirmed_now {
                Authorization::Allow
            } else {
                Authorization::NeedConfirm
            }
        }
        Grade::T2Confirm => {
            if confirmed_now {
                Authorization::Allow
            } else {
                Authorization::NeedConfirm
            }
        }
        Grade::T3Refuse => Authorization::Refuse,
    };
    (a, grade)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(scope: Scope, touches: Touches) -> ActionSpec {
        ActionSpec::new(1, "act", scope, touches)
    }

    #[test]
    fn read_only_is_t0() {
        assert_eq!(grade_of(&spec(Scope::ReadOnly, Touches::Nothing)), Grade::T0Read);
        // 即便标了本应用文件，只读作用域仍是 T0
        assert_eq!(grade_of(&spec(Scope::ReadOnly, Touches::OwnFiles)), Grade::T0Read);
    }

    #[test]
    fn own_files_reversible_is_t1() {
        assert_eq!(grade_of(&spec(Scope::SelfApp, Touches::OwnFiles)), Grade::T1LowRisk);
    }

    #[test]
    fn network_or_system_is_t2() {
        assert_eq!(grade_of(&spec(Scope::SelfApp, Touches::NetworkConfig)), Grade::T2Confirm);
        assert_eq!(grade_of(&spec(Scope::System, Touches::OwnFiles)), Grade::T2Confirm);
    }

    #[test]
    fn redlines_are_t3() {
        assert_eq!(grade_of(&spec(Scope::SelfApp, Touches::UserData)), Grade::T3Refuse);
        assert_eq!(grade_of(&spec(Scope::System, Touches::OtherKernel)), Grade::T3Refuse);
        assert_eq!(grade_of(&spec(Scope::System, Touches::Security)), Grade::T3Refuse);
    }

    #[test]
    fn irreversible_or_no_rollback_downgrades_to_t3() {
        let mut s = spec(Scope::SelfApp, Touches::OwnFiles);
        s.reversible = false;
        assert_eq!(grade_of(&s), Grade::T3Refuse, "不可逆 → 拒绝");
        let mut s2 = spec(Scope::SelfApp, Touches::OwnFiles);
        s2.has_rollback = false;
        assert_eq!(grade_of(&s2), Grade::T3Refuse, "未声明回滚 → 拒绝");
        // 红线 + 可逆 仍是拒绝（红线优先）
        let mut s3 = spec(Scope::SelfApp, Touches::UserData);
        s3.reversible = true;
        s3.has_rollback = true;
        assert_eq!(grade_of(&s3), Grade::T3Refuse);
    }

    #[test]
    fn t0_needs_no_confirmation() {
        let s = spec(Scope::ReadOnly, Touches::Nothing);
        assert_eq!(authorize(&s, &Grants::new(), false).0, Authorization::Allow);
    }

    #[test]
    fn t1_memory_then_revocable() {
        let s = spec(Scope::SelfApp, Touches::OwnFiles);
        let mut g = Grants::new();
        assert_eq!(authorize(&s, &g, false).0, Authorization::NeedConfirm, "未授权需确认");
        assert_eq!(authorize(&s, &g, true).0, Authorization::Allow, "**首次确认即放行**");
        g.grant(s.id);
        assert_eq!(authorize(&s, &g, false).0, Authorization::Allow, "授权后免确认");
        g.revoke(s.id);
        assert_eq!(authorize(&s, &g, false).0, Authorization::NeedConfirm, "撤销后回到逐次确认");
        assert_eq!(authorize(&s, &g, true).0, Authorization::Allow, "撤销后仍可逐次确认执行");
    }

    #[test]
    fn t2_requires_confirmation_every_time() {
        let s = spec(Scope::SelfApp, Touches::NetworkConfig);
        let mut g = Grants::new();
        g.grant(s.id); // 即便"记忆授权"过
        assert_eq!(authorize(&s, &g, false).0, Authorization::NeedConfirm, "T2 不受记忆授权影响");
        assert_eq!(authorize(&s, &g, true).0, Authorization::Allow, "本次确认才放行");
    }

    #[test]
    fn t3_refuses_even_when_confirmed_and_granted() {
        let s = spec(Scope::System, Touches::UserData);
        let mut g = Grants::new();
        g.grant(s.id);
        assert_eq!(authorize(&s, &g, true).0, Authorization::Refuse, "T3 与授权/确认无关");
    }

    #[test]
    fn grade_codes_are_stable() {
        assert_eq!(Grade::T0Read.code(), 0);
        assert_eq!(Grade::T1LowRisk.code(), 1);
        assert_eq!(Grade::T2Confirm.code(), 2);
        assert_eq!(Grade::T3Refuse.code(), 3);
        assert!(Grade::T3Refuse.label().starts_with("T3"));
    }

    #[test]
    fn grants_are_idempotent() {
        let mut g = Grants::new();
        g.grant(7);
        g.grant(7);
        assert_eq!(g.granted.len(), 1);
        g.revoke(7);
        g.revoke(7);
        assert_eq!(g.revoked.len(), 1);
    }
}
