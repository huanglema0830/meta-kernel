//! # 显化生命周期状态机（L4 §3，纯函数）
//!
//! 状态集：`0`（锚点态）∪ `{10..=99}`（十位=圈层 1..9，个位=带内微步 0..9）。
//! 圈层名与微步语义见 `namer::Namer::band_of`；转移规则（L4 §3.3）：
//! - `0` + Awaken → 10（点亮，轮次 +1 由调用方记账）
//! - 同带 Awaken → 个位 +1（19→20 进位）；跨带需**连续 ≥2 Awaken**（防抖）
//! - Settle → 个位 -1（≥10 兜底）；跨带回退亦需连续 ≥2 Settle
//! - `99` 收到连续 N（默认 8）个无 Awaken 事件（Hold/Settle）→ 回融 0
//! - Reset 任意态 → 0（早退回融）
//!
//! 纯函数：`step(state, event, since_last_awaken)` —— 相同输入恒同输出，可单测。

/// 99 极显后的静默回融窗（N 个非 Awaken 事件后回归 0）。
pub const RETREAT_WINDOW: u32 = 8;
/// 跨带所需连续同向事件数（防抖）。
pub const BAND_DEBOUNCE: u32 = 2;

/// 生命周期引擎：内部仅存当前状态与连续计数，逻辑全在纯函数 `step`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleEngine {
    /// 当前状态（0 或 10..=99）。
    pub state: u16,
    /// 自最近一次 Awaken 以来的连续非 Awaken 事件计数（99 静默窗用）。
    pub since_awaken: u32,
    /// 带内推进计数（跨带防抖：连续 Awaken ≥2 才进位；连续 Settle ≥2 才退带）。
    pub band_pending: i32,
    /// 已完成的显化轮次（99→0 计数由调用方经 [`LifecycleEngine::note_round`] 维护）。
    pub rounds: u32,
}

/// 归一内核事件（L4 §3.3 表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelEvent {
    /// 点亮/正向推进（StateChanged→低 code、Compound、Resonance、Self 升）
    Awaken,
    /// 保持（粒子成形、无变化）
    Hold,
    /// 回落（StateChanged→高 code、LowEnergy、Habit 重复）
    Settle,
    /// 回归（entropy→1 或显式 reset）
    Reset,
}

impl LifecycleEngine {
    pub fn new() -> Self {
        Self { state: 0, since_awaken: 0, band_pending: 0, rounds: 0 }
    }

    /// 纯函数转移核心（不修改 self；返回新状态与标志）。
    /// 返回 (新状态, 事件被消费为 True 的动作标志：cross_band/retreat)。
    #[allow(clippy::type_complexity)]
    pub fn step(state: u16, since_awaken: u32, band_pending: i32, ev: KernelEvent) -> (u16, u32, i32, bool) {
        match ev {
            KernelEvent::Reset => (0, 0, 0, false),
            KernelEvent::Awaken => {
                if state == 0 {
                    return (10, 0, 0, false); // 点亮
                }
                let band = state / 10;
                let step_in_band = state % 10;
                if band >= 9 && step_in_band >= 9 {
                    // 99：不越出，等待静默窗回融；复位防抖计数
                    return (99, 0, 0, false);
                }
                // 带内个位 <9 → 直接 +1；个位=9 → 需要跨带防抖
                if step_in_band < 9 {
                    return (state + 1, 0, 0, false);
                }
                // 个位=9，跨带：连续 Awaken ≥2 才进位（如 19→20）
                let pending = band_pending + 1;
                if pending >= BAND_DEBOUNCE as i32 {
                    let next_band = band + 1;
                    let ns = if next_band > 9 { 99 } else { next_band * 10 };
                    (ns, 0, 0, true)
                } else {
                    (state, 0, pending, false)
                }
            }
            KernelEvent::Hold | KernelEvent::Settle => {
                if state == 0 {
                    return (0, 0, 0, false); // 未点亮：保持锚点
                }
                if state == 99 {
                    // 极显静默窗
                    let s = since_awaken + 1;
                    if s >= RETREAT_WINDOW {
                        return (0, 0, 0, true); // 回融
                    }
                    return (99, s, 0, false);
                }
                // 非 99：Hold 只清带内正向防抖；Settle 退档
                match ev {
                    KernelEvent::Settle => {
                        let step_in_band = state % 10;
                        let band = state / 10;
                        if step_in_band > 0 {
                            return (state - 1, 0, 0, false);
                        }
                        // 个位=0 退带（需连续 ≥2 Settle 防抖；band=1 时退到 0=早退回融）
                        let pending = band_pending - 1;
                        if pending <= -(BAND_DEBOUNCE as i32) {
                            if band <= 1 {
                                return (0, 0, 0, false); // 早退回融
                            }
                            return ((band - 1) * 10 + 9, 0, 0, true);
                        }
                        (state, 0, pending, false)
                    }
                    _ => (state, 0, 0, false), // Hold：仅清正向 pending
                }
            }
        }
    }

    /// 应用一个事件（维护内部计数；可溯源调用方自行记录 source）。
    pub fn apply(&mut self, ev: KernelEvent) -> bool {
        let (ns, sa, bp, _flag) = Self::step(self.state, self.since_awaken, self.band_pending, ev);
        let changed = ns != self.state;
        if changed && ns == 0 && self.state == 99 {
            self.rounds += 1; // 一轮 99→0 完成
        }
        self.state = ns;
        self.since_awaken = sa;
        self.band_pending = bp;
        changed
    }
}

impl Default for LifecycleEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn void_awakens_to_10() {
        assert_eq!(LifecycleEngine::step(0, 0, 0, KernelEvent::Awaken).0, 10);
        // Reset/Hold/Settle 在锚点不点亮
        assert_eq!(LifecycleEngine::step(0, 0, 0, KernelEvent::Reset).0, 0);
        assert_eq!(LifecycleEngine::step(0, 0, 0, KernelEvent::Hold).0, 0);
        assert_eq!(LifecycleEngine::step(0, 0, 0, KernelEvent::Settle).0, 0);
    }

    #[test]
    fn same_band_advances_one_per_awaken() {
        // 12 → 13 → …19 逐个推进
        let mut s = 10;
        for expect in [11u16, 12, 13, 14, 15, 16, 17, 18, 19] {
            s = LifecycleEngine::step(s, 0, 0, KernelEvent::Awaken).0;
            assert_eq!(s, expect);
        }
        // 19 → 20（跨带）需两次 Awaken
        let (s1, _, p1, _) = LifecycleEngine::step(19, 0, 0, KernelEvent::Awaken);
        assert_eq!((s1, p1), (19, 1), "第一次不跨带，记录 pending");
        let (s2, _, _, flag) = LifecycleEngine::step(s1, 0, p1, KernelEvent::Awaken);
        assert_eq!((s2, flag), (20, true), "第二次跨带进位");
    }

    #[test]
    fn settle_retreats_within_band_and_cross_band_debounced() {
        // 带内回退
        let (s1, _, _, _) = LifecycleEngine::step(14, 0, 0, KernelEvent::Settle);
        assert_eq!(s1, 13);
        // 个位 0 退带需两次
        let (s1, _, p1, _) = LifecycleEngine::step(20, 0, 0, KernelEvent::Settle);
        assert_eq!((s1, p1), (20, -1));
        let (s2, _, _, flag) = LifecycleEngine::step(20, 0, p1, KernelEvent::Settle);
        assert_eq!((s2, flag), (19, true));
        // band=1 时个位 0 连续两次 → 早退回融 0
        let (s1, _, p1, _) = LifecycleEngine::step(10, 0, 0, KernelEvent::Settle);
        assert_eq!((s1, p1), (10, -1));
        let (s2, _, _, _) = LifecycleEngine::step(10, 0, p1, KernelEvent::Settle);
        assert_eq!(s2, 0);
    }

    #[test]
    fn retreat_window_returns_99_to_0_after_n_holds() {
        // 99 需要 RETREAT_WINDOW(8) 个非 Awaken
        let (mut s, mut sa) = (99u16, 0u32);
        let mut retreated = false;
        for i in 1..=RETREAT_WINDOW + 2 {
            let (ns, nsa, _, _flag) = LifecycleEngine::step(s, sa, 0, KernelEvent::Hold);
            if ns == 0 {
                retreated = true;
                assert_eq!(i, RETREAT_WINDOW, "恰在第 N 个非 Awaken 回融");
                break;
            }
            s = ns;
            sa = nsa;
        }
        assert!(retreated, "应在静默窗后回融 0");
        // Awaken 可重置静默窗（99 被再次点亮推进——保持 99）
        let (ns, nsa, _, _) = LifecycleEngine::step(99, 4, 0, KernelEvent::Awaken);
        assert_eq!((ns, nsa), (99, 0), "99 收到 Awaken 复位静默计数");
    }

    #[test]
    fn apply_tracks_rounds_on_retreat() {
        let mut eng = LifecycleEngine::new();
        eng.apply(KernelEvent::Awaken);
        assert_eq!(eng.state, 10);
        // 冲到 99
        let mut guard = 0;
        while eng.state != 99 && guard < 300 {
            eng.apply(KernelEvent::Awaken);
            guard += 1;
        }
        assert_eq!(eng.state, 99, "应到达极显");
        assert_eq!(eng.rounds, 0);
        // 静默窗回融 → 轮次 +1
        guard = 0;
        while eng.state != 0 && guard < 30 {
            eng.apply(KernelEvent::Hold);
            guard += 1;
        }
        assert_eq!(eng.state, 0);
        assert_eq!(eng.rounds, 1, "99→0 完成记一轮");
        // 再点亮进入新一轮
        eng.apply(KernelEvent::Awaken);
        assert_eq!(eng.state, 10);
    }

    #[test]
    fn reset_from_any_state_returns_to_void() {
        assert_eq!(LifecycleEngine::step(55, 0, 0, KernelEvent::Reset).0, 0);
        assert_eq!(LifecycleEngine::step(10, 0, 0, KernelEvent::Reset).0, 0);
    }
}
