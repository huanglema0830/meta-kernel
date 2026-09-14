//! L7 · 修复建议（**内核侧只出建议 + 分级 + 账本，不执行任何写操作**）。
//!
//! 依据：`docs/L7_EXECUTION_DESIGN.md §7`（方案 A 架构 + 首版只开放 T1，发起人 v0.109 裁决 Q1–Q4）。
//!
//! 分工：
//! - **内核侧（本模块）**：`Symptom`（症状）→ `Suggestion`（候选动作）→ `RepairPlan`（分级后的计划）；
//!   并把「建议/确认/执行/验证/回滚」记入 [`ActionLedger`]。
//! - **宿主侧**：真正执行（`npb-gateway/src/l7_exec.rs`），只接受**预置动作 id**。
//!
//! **硬约束（内核侧同样守）**：动作来自**编译期常量目录** [`CATALOG`]，**不存在"自由文本动作"**；
//! 未在目录中的动作一律无法表达（类型系统层面就不可能）。

use crate::l7::grade::{authorize, ActionSpec, Authorization, Grade, Grants, Scope, Touches};
use crate::l7::ledger::ActionLedger;

/// 预置动作（**编译期常量目录**；宿主侧按 `key`/`id` 对表执行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionDef {
    pub id: u32,
    /// 稳定键（宿主与 UI 用的唯一标识；非自由文本）。
    pub key: &'static str,
    pub name: &'static str,
    /// 回滚说明（`"—"` 表示无状态变更、无需回滚）。
    pub rollback_hint: &'static str,
    pub spec: ActionSpec,
}

/// **首批 T1 动作白名单**（发起人 Q3 确认的四项）。
pub const CATALOG: [ActionDef; 4] = [
    ActionDef {
        id: 1,
        key: "clean-temp",
        name: "清理本应用临时文件",
        rollback_hint: "从执行前备份恢复被清理的临时文件",
        spec: ActionSpec { id: 1, name: "清理本应用临时文件", scope: Scope::SelfApp, touches: Touches::OwnFiles, reversible: true, has_rollback: true },
    },
    ActionDef {
        id: 2,
        key: "restart-watchdog",
        name: "重启看门狗",
        rollback_hint: "—（无持久状态变更；再次重启即可）",
        spec: ActionSpec { id: 2, name: "重启看门狗", scope: Scope::SelfApp, touches: Touches::OwnFiles, reversible: true, has_rollback: true },
    },
    ActionDef {
        id: 3,
        key: "reload-config",
        name: "重读配置",
        rollback_hint: "—（只读 + 记录，不改配置内容）",
        spec: ActionSpec { id: 3, name: "重读配置", scope: Scope::SelfApp, touches: Touches::OwnFiles, reversible: true, has_rollback: true },
    },
    ActionDef {
        id: 4,
        key: "trigger-probe",
        name: "触发探针采集",
        rollback_hint: "—（只读采集）",
        spec: ActionSpec { id: 4, name: "触发探针采集", scope: Scope::SelfApp, touches: Touches::OwnFiles, reversible: true, has_rollback: true },
    },
];

/// 按 id 查预置动作（**找不到即 None → 宿主必须拒绝**）。
pub fn action_by_id(id: u32) -> Option<ActionDef> {
    CATALOG.iter().copied().find(|a| a.id == id)
}

/// 按 key 查预置动作。
pub fn action_by_key(key: &str) -> Option<ActionDef> {
    CATALOG.iter().copied().find(|a| a.key == key)
}

/// 症状（诊断侧的输入抽象；由宿主把 L5 结论映射为症状）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symptom {
    /// 探针数据陈旧（久未上报）。
    StaleProbe,
    /// 看门狗不在。
    WatchdogDown,
    /// 配置/资源清单与实际不一致。
    ConfigDrift,
    /// 本应用临时件堆积。
    TempBloat,
    /// 未知症状（**不猜**——不产生任何建议）。
    Unknown,
}

/// 一条建议（动作 + 理由）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub action: ActionDef,
    pub reason: &'static str,
}

/// 修复计划（分级后的候选动作）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepairPlan {
    /// 可进入授权流程的建议（T0/T1/T2）。
    pub suggestions: Vec<Suggestion>,
    /// 因分级被剔除的动作（T3 → 永不执行；如实记录原因）。
    pub refused: Vec<(&'static str, &'static str)>,
}

/// **症状 → 候选动作**（规则映射；一个症状一条建议，不做无依据的联想）。
pub fn suggest(s: Symptom) -> Option<Suggestion> {
    let (key, reason): (&str, &'static str) = match s {
        Symptom::StaleProbe => ("trigger-probe", "探针数据陈旧：触发一次采集以刷新本底"),
        Symptom::WatchdogDown => ("restart-watchdog", "看门狗不在：重启以恢复崩溃自愈"),
        Symptom::ConfigDrift => ("reload-config", "资源清单与实际不一致：重读配置"),
        Symptom::TempBloat => ("clean-temp", "本应用临时件堆积：清理以释放空间"),
        Symptom::Unknown => return None, // 不猜、不动
    };
    action_by_key(key).map(|action| Suggestion { action, reason })
}

/// **生成修复计划**：把症状列表映射为建议，并**逐条过分级**（T3 剔除并如实记录）。
pub fn plan(symptoms: &[Symptom]) -> RepairPlan {
    let mut out = RepairPlan::default();
    for s in symptoms {
        if let Some(sg) = suggest(*s) {
            let g = crate::l7::grade::grade_of(&sg.action.spec);
            if matches!(g, Grade::T3Refuse) {
                out.refused.push((sg.action.key, "分级为 T3（不可逆/越界）→ 不进入候选"));
                continue;
            }
            if !out.suggestions.iter().any(|x| x.action.id == sg.action.id) {
                out.suggestions.push(sg);
            }
        }
    }
    out
}

/// 一次"判定 + 记账"的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub authorization: Authorization,
    pub grade: Grade,
    pub hash: u64,
}

/// **判定并记账**（内核侧闭环的"账"部分）：
/// - 未授权 → `NeedConfirm`，账本记 `suggested`；
/// - 本次已确认 → `Allow`，账本记 `confirmed`；
/// - 已记忆授权且未撤销 → `Allow`，账本记 `granted`；
/// - T3 → `Refuse`，账本记 `refused`（**同样入链**）。
pub fn decide_and_record(
    ledger: &mut ActionLedger,
    grants: &Grants,
    action: &ActionDef,
    confirmed_now: bool,
) -> Decision {
    let (a, g) = authorize(&action.spec, grants, confirmed_now);
    let outcome: &'static str = match a {
        Authorization::Refuse => "refused",
        Authorization::Allow => {
            if confirmed_now {
                "confirmed"
            } else {
                "granted"
            }
        }
        Authorization::NeedConfirm => "suggested",
    };
    let hash = ledger.append(action.id, g, outcome, action.name);
    Decision { authorization: a, grade: g, hash }
}

/// 执行结果记账（宿主回调后由宿主调用；内核侧只提供纯逻辑）。
pub fn record_outcome(
    ledger: &mut ActionLedger,
    action: &ActionDef,
    outcome: &'static str,
    note: &'static str,
) -> u64 {
    ledger.append(action.id, crate::l7::grade::grade_of(&action.spec), outcome, note)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **目录即白名单**：4 个 T1 动作齐备且 id/key 唯一。
    #[test]
    fn catalog_is_the_whitelist() {
        assert_eq!(CATALOG.len(), 4);
        let keys: Vec<&str> = CATALOG.iter().map(|a| a.key).collect();
        assert_eq!(keys, vec!["clean-temp", "restart-watchdog", "reload-config", "trigger-probe"]);
        for (i, a) in CATALOG.iter().enumerate() {
            assert_eq!(a.id as usize, i + 1, "id 连续（宿主按 id 对表）");
        }
        // 全部为 T1
        for a in CATALOG.iter() {
            assert_eq!(crate::l7::grade::grade_of(&a.spec), Grade::T1LowRisk, "{} 应为 T1", a.key);
        }
    }

    #[test]
    fn lookup_rejects_unknown_ids_and_keys() {
        assert!(action_by_id(1).is_some());
        assert!(action_by_id(0).is_none(), "id 0 不在目录");
        assert!(action_by_id(99).is_none(), "未知 id → 必须拒绝");
        assert!(action_by_key("rm-rf").is_none(), "自由文本键 → 拒绝");
        assert!(action_by_key("").is_none());
        assert_eq!(action_by_key("clean-temp").unwrap().id, 1);
    }

    #[test]
    fn suggest_maps_symptoms_one_to_one() {
        assert_eq!(suggest(Symptom::StaleProbe).unwrap().action.key, "trigger-probe");
        assert_eq!(suggest(Symptom::WatchdogDown).unwrap().action.key, "restart-watchdog");
        assert_eq!(suggest(Symptom::ConfigDrift).unwrap().action.key, "reload-config");
        assert_eq!(suggest(Symptom::TempBloat).unwrap().action.key, "clean-temp");
        assert!(suggest(Symptom::Unknown).is_none(), "未知症状不猜、不动");
    }

    #[test]
    fn plan_dedups_and_keeps_only_catalog_actions() {
        let p = plan(&[Symptom::TempBloat, Symptom::TempBloat, Symptom::StaleProbe]);
        assert_eq!(p.suggestions.len(), 2, "同动作去重");
        assert!(p.refused.is_empty(), "T1 计划不应有拒绝项");
        assert!(p.suggestions.iter().all(|s| action_by_id(s.action.id).is_some()));
    }

    #[test]
    fn plan_with_only_unknown_is_empty_and_safe() {
        let p = plan(&[Symptom::Unknown]);
        assert!(p.suggestions.is_empty());
        assert!(p.refused.is_empty());
    }

    /// **首次授权后免确认，可撤销**（验收项之一）。
    #[test]
    fn first_confirm_then_memory_then_revocable() {
        let mut ledger = ActionLedger::new();
        let a = action_by_id(1).unwrap();
        let mut g = Grants::new();

        // ① 首次：需确认 → 记 suggested
        let d1 = decide_and_record(&mut ledger, &g, &a, false);
        assert_eq!(d1.authorization, Authorization::NeedConfirm);
        assert_eq!(ledger.links[0].outcome, "suggested");

        // ② 本次确认执行 → 记 confirmed，并写入记忆授权
        let d2 = decide_and_record(&mut ledger, &g, &a, true);
        assert_eq!(d2.authorization, Authorization::Allow);
        assert_eq!(ledger.links[1].outcome, "confirmed");
        g.grant(a.id);

        // ③ 之后免确认 → 记 granted
        let d3 = decide_and_record(&mut ledger, &g, &a, false);
        assert_eq!(d3.authorization, Authorization::Allow);
        assert_eq!(ledger.links[2].outcome, "granted");

        // ④ 撤销 → 回到需确认
        g.revoke(a.id);
        let d4 = decide_and_record(&mut ledger, &g, &a, false);
        assert_eq!(d4.authorization, Authorization::NeedConfirm, "撤销后需重新确认");
        assert_eq!(ledger.links[3].outcome, "suggested");
        assert!(ledger.verify(), "全程入链且链完整");
    }

    /// **T3 永不放行**：把目录动作人为改成 T3，确认也无法放行。
    #[test]
    fn t3_action_can_never_be_authorized() {
        let mut ledger = ActionLedger::new();
        let mut a = action_by_id(1).unwrap();
        a.spec.touches = Touches::UserData; // 人为越界
        assert_eq!(crate::l7::grade::grade_of(&a.spec), Grade::T3Refuse);
        let mut g = Grants::new();
        g.grant(a.id);
        let d = decide_and_record(&mut ledger, &g, &a, true);
        assert_eq!(d.authorization, Authorization::Refuse, "T3 与授权/确认无关");
        assert_eq!(ledger.links[0].outcome, "refused");
        assert!(ledger.verify());
    }

    #[test]
    fn outcomes_are_recorded_with_owner_visible_names() {
        let mut ledger = ActionLedger::new();
        let a = action_by_id(2).unwrap();
        record_outcome(&mut ledger, &a, "executed", "重启看门狗");
        record_outcome(&mut ledger, &a, "verified", "健康检查通过");
        record_outcome(&mut ledger, &a, "rolled_back", "从备份恢复");
        assert!(ledger.has_outcome("executed"));
        assert!(ledger.has_outcome("verified"));
        assert!(ledger.has_outcome("rolled_back"));
        assert!(ledger.verify());
        let txt = ledger.to_text("[元内核]");
        assert!(txt.contains("重启看门狗"));
        assert_eq!(ledger.of_action(2).len(), 3);
    }
}
