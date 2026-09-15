//! L5 · **注意力—渲染—探测 联动**（l5_attention）。
//!
//! 依据发起人 v0.123 ③：形成 **"注意力—渲染—探测"完整闭环**。
//!
//! - 紧张高 → **LOD 渲染更多细节**，**探测策略更主动**；
//! - 平静高 → **LOD 渲染更少细节**，**探测策略更被动**。
//!
//! ## 为什么 LOD 与探测必须**同源**
//! 若两者各用一套阈值，必然出现"要高细节却同时很被动"这类**自相矛盾**的状态。
//! 故本模块把"注意力分配"压缩成**一个标量** `activity`，由它同时决定渲染精度与探测门槛——
//! **同源才叫闭环**，不同源只是两条并列的规则。
//!
//! `activity = clamp(0.6·紧张 + 0.4·(1 − 平静), 0, 1)`
//! （紧张＝需求未满足的累积张力；不平静＝缺乏稳定基线 → 两者都需要更多注意力）
//!
//! ## 探测联动的方向（**实测修正过一版**）
//! 探测**只由"置信度门槛"这一个旋钮**联动，方向是：
//! **注意力高 → 对确信度的要求更高（门槛升）→ 更容易转主动**；
//! 注意力低 → 门槛降 → 更被动。
//!
//! 语义解释：紧张时"看不清"的代价更大，所以稍有不确信就要主动探；
//! 平静时"水面模式"——容忍低确信度，不去打扰。
//!
//! ⚠️ 我第一版把方向设反了（让平静抬高门槛），测试立刻暴露"平静反而是主动"，
//! 已按实测改正——**方向靠测试定，不靠直觉**。
//!
//! ## 纪律
//! - **零依赖**；**不改探测默认被动**（水面模式）：activity 只调**门槛**，
//!   既不取消"感知困难必主动"，也不取消"主动必须带理由"。
//! - `lod_n` 恒为 **2 的幂**（GPU 双调排序要求）。
//! - 门槛区间 `[0.20, 0.60]` **包住** `l5_quad::PROBE_CONFIDENCE_FLOOR`（0.35）——
//!   即把"旧默认策略"作为本联动的一个中间档保留下来。

use crate::l5_quad::{decide_probe_with_floor, ProbeDecision, ProbeMode, ProbeReason, Quad};

/// 可选渲染精度档位（**必须是 2 的幂**：GPU 双调排序要求 n 为 2 的幂）。
pub const LOD_LEVELS: [u32; 4] = [128, 256, 512, 1024];

/// 活动度权重：\[紧张, 不平静\]。
pub const ACTIVITY_WEIGHTS: [f64; 2] = [0.6, 0.4];

/// 档位分界（activity 越过即升档）。
pub const LOD_THRESHOLDS: [f64; 3] = [0.25, 0.50, 0.75];

/// 探测置信度门槛的下/上界：平静端最低（最被动）、紧张端最高（最易主动）。
pub const PROBE_FLOOR_MIN: f64 = 0.20;
pub const PROBE_FLOOR_MAX: f64 = 0.60;

/// **活动度**：把四元组压成一个"需要多少注意力"的标量（0..1）。
pub fn activity_of(q: &Quad) -> f64 {
    let t = if q.tension.is_finite() { q.tension.clamp(0.0, 1.0) } else { 0.0 };
    let c = if q.calm.is_finite() { q.calm.clamp(0.0, 1.0) } else { 0.0 };
    let a = ACTIVITY_WEIGHTS[0] * t + ACTIVITY_WEIGHTS[1] * (1.0 - c);
    if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.0 }
}

/// **活动度 → LOD 档位**（单调不减）。
pub fn lod_n_for_activity(activity: f64) -> u32 {
    let a = if activity.is_finite() { activity.clamp(0.0, 1.0) } else { 0.0 };
    let mut n = LOD_LEVELS[0];
    let mut i = 0usize;
    while i < LOD_THRESHOLDS.len() {
        if a >= LOD_THRESHOLDS[i] {
            n = LOD_LEVELS[i + 1];
            i += 1;
        } else {
            break;
        }
    }
    n
}

/// 四元组 → LOD 档位。
pub fn lod_n_for(q: &Quad) -> u32 {
    lod_n_for_activity(activity_of(q))
}

/// **活动度 → 探测门槛**（线性、单调递增）：
/// 平静端 = [`PROBE_FLOOR_MIN`]（最被动）；紧张端 = [`PROBE_FLOOR_MAX`]（最易主动）。
pub fn probe_floor_for_activity(activity: f64) -> f64 {
    let a = if activity.is_finite() { activity.clamp(0.0, 1.0) } else { 0.0 };
    PROBE_FLOOR_MIN + (PROBE_FLOOR_MAX - PROBE_FLOOR_MIN) * a
}

/// 联动结果（**渲染与探测出自同一 activity**，故二者必然同向）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttentionPlan {
    /// 注意力需求（0..1）——LOD 与探测的共同来源。
    pub activity: f64,
    /// 本帧渲染精度档位（2 的幂）。
    pub lod_n: u32,
    /// 本帧生效的探测门槛。
    pub probe_floor: f64,
    /// 探测决定（默认被动；主动必带理由）。
    pub probe: ProbeDecision,
    /// 主动探测的理由（被动时为 `None`）。
    pub probe_reason: Option<ProbeReason>,
}

impl AttentionPlan {
    /// 档位的中文名（供日志/UI）。
    fn level_label(&self) -> &'static str {
        match self.lod_n {
            n if n <= LOD_LEVELS[0] => "最低",
            n if n <= LOD_LEVELS[1] => "低",
            n if n <= LOD_LEVELS[2] => "高",
            _ => "最高",
        }
    }

    /// 是否处于"高注意力"模式（最高档 LOD）。
    pub fn is_high_attention(&self) -> bool {
        self.lod_n >= LOD_LEVELS[LOD_LEVELS.len() - 1]
    }

    /// 一句话依据（供日志/UI 显示）。
    pub fn detail(&self) -> String {
        format!(
            "活动度 {:.3} → LOD n={}（{} 档）｜探测门槛 {:.3} → {}",
            self.activity,
            self.lod_n,
            self.level_label(),
            self.probe_floor,
            match self.probe.mode {
                ProbeMode::Passive => "被动（水面模式）".to_string(),
                ProbeMode::Active => format!("主动（{:?}）", self.probe.reason),
            }
        )
    }
}

/// **联动主入口**：四元组（注意力）＋ 感知质量（confidence/deviation/stale）→ 渲染档位 + 探测决定。
///
/// 注意：`stale` / `deviation` 这两条**原有纪律**不受 activity 影响——
/// 注意力只调"确信度要多高才算看清"，**不取消**任何一条既有触发条件。
pub fn plan(q: &Quad, deviation: f64, confidence: f64, stale: bool) -> AttentionPlan {
    let activity = activity_of(q);
    let lod_n = lod_n_for_activity(activity);
    let probe_floor = probe_floor_for_activity(activity);
    let probe = decide_probe_with_floor(confidence, deviation, stale, probe_floor);
    AttentionPlan { activity, lod_n, probe_floor, probe, probe_reason: probe.reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l5_quad::PROBE_CONFIDENCE_FLOOR;

    fn q(t: f64, c: f64) -> Quad {
        Quad { tension: t, calm: c, liking: 0.5, safety: 0.5 }
    }

    /// 与 LOD 无关的"中间确信度"：固定它，才能公平比较"探测模式只因注意力而变"。
    const MID_CONF: f64 = 0.40;

    #[test]
    fn activity_is_bounded_and_monotone() {
        assert!((activity_of(&q(0.0, 1.0)) - 0.0).abs() < 1e-12, "全静 → 0");
        assert!((activity_of(&q(1.0, 0.0)) - 1.0).abs() < 1e-12, "全紧 → 1");
        let mut prev = -1.0;
        for i in 0..=10 {
            let a = activity_of(&q(i as f64 / 10.0, 0.5));
            assert!(a >= prev, "紧张↑ → activity 不减");
            prev = a;
        }
        assert!(activity_of(&q(f64::NAN, f64::NAN)).is_finite(), "NaN 不得失控");
    }

    #[test]
    fn lod_monotone_and_power_of_two() {
        let mut prev = 0u32;
        for i in 0..=100 {
            let n = lod_n_for_activity(i as f64 / 100.0);
            assert!(n.is_power_of_two(), "必须是 2 的幂：{n}");
            assert!(n >= prev, "activity↑ → 档位不降");
            prev = n;
        }
        assert_eq!(lod_n_for_activity(0.0), LOD_LEVELS[0]);
        assert_eq!(lod_n_for_activity(1.0), LOD_LEVELS[3]);
    }

    #[test]
    fn probe_floor_direction_is_correct() {
        let calm = probe_floor_for_activity(0.0);
        let tense = probe_floor_for_activity(1.0);
        assert!(tense > calm, "注意力高 → 门槛更高（更易主动）：{} vs {}", tense, calm);
        assert!((calm - PROBE_FLOOR_MIN).abs() < 1e-12);
        assert!((tense - PROBE_FLOOR_MAX).abs() < 1e-12);
        // 门槛区间必须**包住旧默认门槛**：本联动不废弃旧策略，只是把它的位置随注意力移动
        assert!(PROBE_FLOOR_MIN < PROBE_CONFIDENCE_FLOOR);
        assert!(PROBE_FLOOR_MAX > PROBE_CONFIDENCE_FLOOR);
    }

    /// 核心验收①：固定确信度，只有注意力不同 → 紧张更细 + 更主动，平静更粗 + 更被动。
    #[test]
    fn tense_gets_more_detail_and_more_active_probe() {
        let tense = plan(&q(0.95, 0.05), 0.0, MID_CONF, false);
        let calm = plan(&q(0.05, 0.95), 0.0, MID_CONF, false);
        assert!(tense.lod_n > calm.lod_n, "紧张 → 更多细节：{} vs {}", tense.lod_n, calm.lod_n);
        assert_eq!(tense.probe.mode, ProbeMode::Active, "紧张 → 更主动");
        assert_eq!(calm.probe.mode, ProbeMode::Passive, "平静 → 更被动");
        assert!(tense.probe_reason.is_some(), "主动必须带理由");
        assert!(calm.probe_reason.is_none(), "被动不带理由");
        assert_eq!(tense.probe_reason, Some(ProbeReason::LowConfidence));
    }

    /// 核心验收②：LOD 档位与探测模式**严格同向**（同源 → 不会自相矛盾）。
    #[test]
    fn lod_and_probe_are_strictly_synchronised() {
        let mut prev_n = 0u32;
        let mut saw_active = false;
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            let p = plan(&q(t, 1.0 - t), 0.0, MID_CONF, false);
            assert!(p.lod_n >= prev_n, "档位不降（activity={}）", p.activity);
            prev_n = p.lod_n;
            if p.probe.mode == ProbeMode::Active {
                saw_active = true;
            } else if saw_active {
                panic!(
                    "activity 继续升高却退回被动，说明 LOD 与探测不同源（activity={}）",
                    p.activity
                );
            }
        }
        assert!(saw_active, "高 activity 端必须出现过主动探测");
    }

    #[test]
    fn calm_dominant_is_most_passive() {
        let p = plan(&q(0.0, 1.0), 0.0, 0.9, false);
        assert_eq!(p.lod_n, LOD_LEVELS[0], "平静 → 最低档（省资源）");
        assert_eq!(p.probe.mode, ProbeMode::Passive);
        assert!((p.probe_floor - PROBE_FLOOR_MIN).abs() < 1e-12, "最被动时门槛最低");
    }

    #[test]
    fn perception_difficulty_still_wins_over_attention() {
        // 两条**原有纪律**不受 activity 影响：即便完全平静，感知困难也必须主动
        let low = plan(&q(0.0, 1.0), 0.0, 0.05, false);
        assert_eq!(low.probe.mode, ProbeMode::Active, "确信度过低 → 必主动");
        assert_eq!(low.probe_reason, Some(ProbeReason::LowConfidence));
        let dev = plan(&q(0.0, 1.0), 0.9, 0.9, false);
        assert_eq!(dev.probe.mode, ProbeMode::Active, "偏离过大 → 必主动");
        assert_eq!(dev.probe_reason, Some(ProbeReason::HighDeviation));
        let stale = plan(&q(0.0, 1.0), 0.0, 0.9, true);
        assert_eq!(stale.probe.mode, ProbeMode::Active, "信号陈旧 → 必主动");
        assert_eq!(stale.probe_reason, Some(ProbeReason::StaleSignal));
        // 但平静端的最低档 LOD 不变（省资源与"必要时主动"不冲突）
        assert_eq!(low.lod_n, LOD_LEVELS[0]);
    }

    #[test]
    fn detail_is_human_readable() {
        let p = plan(&q(0.9, 0.1), 0.0, 0.3, false);
        let d = p.detail();
        assert!(d.contains("活动度") && d.contains("LOD n="), "{d}");
        assert!(d.contains("主动"), "{d}");
        assert!(p.is_high_attention());
    }
}
