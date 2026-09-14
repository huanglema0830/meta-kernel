//! L7 · 动作账（**哈希链记录**）。
//!
//! 依据：`docs/L7_EXECUTION_DESIGN.md §2.1⑥ / §2.4`。每一次**建议/确认/执行/验证**都追加一条链节，
//! 链节前向哈希（`hash = fnv1a(prev ‖ action_id ‖ grade ‖ seq ‖ outcome ‖ note)`）——
//! **任何历史被改都会失配**，用于事后复核"到底执行过什么"。
//!
//! 说明（分账原则）：本账记录**动作**（execution），与基因库的**验证记录**（公式验证）分账；
//! 两者**同构**（同哈希函数 `gene_library::fnv1a64`、同前向链语义），但**不混用**——
//! 公式账回答"规律是否被验证"，动作账回答"我们做过什么"。

use crate::gene_library::fnv1a64;
use crate::l7::grade::Grade;

/// 链首初始状态。
pub const GENESIS: u64 = 0;

/// 一条动作链节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerLink {
    pub seq: u32,
    pub prev: u64,
    pub hash: u64,
    pub action_id: u32,
    pub grade: Grade,
    /// 结果（`suggested` / `confirmed` / `executed` / `verified` / `refused` / `rolled_back`）。
    pub outcome: &'static str,
    pub note: &'static str,
}

/// 动作账。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionLedger {
    pub links: Vec<LedgerLink>,
}

fn link_hash(prev: u64, action_id: u32, grade: Grade, seq: u32, outcome: &str, note: &str) -> u64 {
    let mut h = fnv1a64(GENESIS ^ 0x9e37_79b9_7f4a_7c15, &prev.to_le_bytes());
    h = fnv1a64(h, &action_id.to_le_bytes());
    h = fnv1a64(h, &[grade.code()]);
    h = fnv1a64(h, &seq.to_le_bytes());
    h = fnv1a64(h, outcome.as_bytes());
    h = fnv1a64(h, note.as_bytes());
    h
}

impl ActionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条链节，返回本条哈希。
    pub fn append(
        &mut self,
        action_id: u32,
        grade: Grade,
        outcome: &'static str,
        note: &'static str,
    ) -> u64 {
        let seq = self.links.len() as u32 + 1;
        let prev = self.head();
        let hash = link_hash(prev, action_id, grade, seq, outcome, note);
        self.links.push(LedgerLink { seq, prev, hash, action_id, grade, outcome, note });
        hash
    }

    /// 链锚（最新哈希；空账为 [`GENESIS`]）。
    pub fn head(&self) -> u64 {
        self.links.last().map(|l| l.hash).unwrap_or(GENESIS)
    }

    pub fn len(&self) -> usize {
        self.links.len()
    }

    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// 校验链完整性（序号连续 / 前向哈希一致 / 本条哈希可复算）。
    pub fn verify(&self) -> bool {
        let mut prev = GENESIS;
        for (i, l) in self.links.iter().enumerate() {
            if l.seq as usize != i + 1 || l.prev != prev {
                return false;
            }
            let expect = link_hash(prev, l.action_id, l.grade, l.seq, l.outcome, l.note);
            if expect != l.hash {
                return false;
            }
            prev = l.hash;
        }
        true
    }

    /// 某动作的全部记录（按顺序）。
    pub fn of_action(&self, action_id: u32) -> Vec<LedgerLink> {
        self.links.iter().filter(|l| l.action_id == action_id).copied().collect()
    }

    /// 是否出现过该结果（如 `"refused"`）。
    pub fn has_outcome(&self, outcome: &str) -> bool {
        self.links.iter().any(|l| l.outcome == outcome)
    }

    /// 纯文本（每行带归属前缀，供运行日志/审计阅读）。
    pub fn to_text(&self, owner_tag: &str) -> String {
        let mut s = String::new();
        for l in &self.links {
            s.push_str(&format!(
                "{} AUDIT #{} {} {} · {} (prev={:#x})\n",
                owner_tag, l.seq, l.grade.label(), l.outcome, l.note, l.prev
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_ledger_is_valid() {
        let l = ActionLedger::new();
        assert!(l.is_empty());
        assert_eq!(l.head(), GENESIS);
        assert!(l.verify());
    }

    #[test]
    fn append_chains_and_verifies() {
        let mut l = ActionLedger::new();
        let h1 = l.append(1, Grade::T1LowRisk, "suggested", "清理临时文件");
        let h2 = l.append(1, Grade::T1LowRisk, "confirmed", "用户确认");
        let h3 = l.append(1, Grade::T1LowRisk, "executed", "已执行");
        assert_eq!(l.len(), 3);
        assert_ne!(h1, h2);
        assert_ne!(h2, h3);
        assert_eq!(l.links[1].prev, h1, "前向链接续");
        assert_eq!(l.head(), h3);
        assert!(l.verify());
    }

    #[test]
    fn tampering_is_detected() {
        let mut l = ActionLedger::new();
        l.append(1, Grade::T1LowRisk, "executed", "a");
        l.append(2, Grade::T2Confirm, "executed", "b");
        l.append(3, Grade::T3Refuse, "refused", "c");
        assert!(l.verify());

        // 改备注
        let mut t1 = l.clone();
        t1.links[1].note = "伪造";
        assert!(!t1.verify(), "改历史必须可检出");
        // 改结果
        let mut t2 = l.clone();
        t2.links[0].outcome = "refused";
        assert!(!t2.verify());
        // 改等级（把拒绝改成放行是最危险的伪造）
        let mut t3 = l.clone();
        t3.links[2].grade = Grade::T0Read;
        assert!(!t3.verify(), "降级伪造必须可检出");
        // 改哈希本身
        let mut t4 = l.clone();
        t4.links[0].hash = 42;
        assert!(!t4.verify());
        // 删中间
        let mut t5 = l.clone();
        t5.links.remove(1);
        assert!(!t5.verify(), "删链节必须可检出");
    }

    #[test]
    fn of_action_filters() {
        let mut l = ActionLedger::new();
        l.append(1, Grade::T1LowRisk, "executed", "a");
        l.append(2, Grade::T2Confirm, "confirmed", "b");
        l.append(1, Grade::T1LowRisk, "verified", "c");
        let a1 = l.of_action(1);
        assert_eq!(a1.len(), 2);
        assert_eq!(a1[0].outcome, "executed");
        assert_eq!(a1[1].outcome, "verified");
        assert!(l.of_action(99).is_empty());
    }

    #[test]
    fn refused_recorded_and_queryable() {
        let mut l = ActionLedger::new();
        l.append(9, Grade::T3Refuse, "refused", "触及用户数据");
        assert!(l.has_outcome("refused"));
        assert!(!l.has_outcome("executed"));
        assert!(l.verify(), "拒绝记录同样入链");
    }

    #[test]
    fn text_output_carries_owner_prefix_and_chain_info() {
        let mut l = ActionLedger::new();
        l.append(1, Grade::T2Confirm, "executed", "升级本应用");
        let t = l.to_text("[元内核]");
        assert!(t.starts_with("[元内核] AUDIT #1 T2 逐次确认 executed"), "{t}");
        assert!(t.contains("prev=0x"), "含前向锚（可追溯）");
    }
}
