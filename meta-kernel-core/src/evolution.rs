//! # 进化过程记录与回放（Evolution）
//!
//! 系统呈现**进化过程**而非仅进化结果（3.3）：
//! - 时间线底层 = **步数**（自然数递增）；斐波那契是自然数的一种进化模式；
//! - 每次步进对应一个"因果链"事件：物态切换 / 化合发生 / 粒子成形 / 结晶形成；
//! - `EvolutionLog` 记录全部事件，`replay()` 供按步回放；界面侧提供回放控制条。

use crate::interference::Particle;
use crate::state::State;

/// 事件类型。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventKind {
    /// 物态切换（能量→气→液→固 或 反向）。
    StateChange { from: State, to: State },
    /// 化合发生（创新增量）。
    Compound { innovation: f32 },
    /// 驻点粒子成形（波粒二象性：波动冻结为实体）。
    Particle { layer: u8 },
    /// 结晶形成（结构固化事件）。
    Crystallize { strength: f32 },
}

/// 时间线事件（step 为因果链锚点）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvolutionEvent {
    pub step: u64,
    pub kind: EventKind,
}

/// 进化时间线。
#[derive(Debug, Clone, Default)]
pub struct EvolutionLog {
    events: Vec<EvolutionEvent>,
    next_step: u64,
}

impl EvolutionLog {
    /// 时间线上限（环形）。
    pub const CAP: usize = 1024;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, kind: EventKind) {
        self.events.push(EvolutionEvent { step: self.next_step, kind });
        if self.events.len() > Self::CAP {
            self.events.remove(0);
        }
    }

    pub fn record_state_change(&mut self, from: State, to: State) {
        if from != to {
            self.record(EventKind::StateChange { from, to });
        }
    }

    pub fn record_compound(&mut self, innovation: f32) {
        self.record(EventKind::Compound { innovation });
    }

    pub fn record_particle(&mut self, p: &Particle) {
        self.record(EventKind::Particle { layer: p.layer });
    }

    pub fn record_crystallize(&mut self, strength: f32) {
        self.record(EventKind::Crystallize { strength });
    }

    /// 因果链步进：每次调用使时间线前进 1（步数 = 自然数递增）。
    pub fn tick(&mut self) {
        self.next_step += 1;
    }

    pub fn step_now(&self) -> u64 {
        self.next_step
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 当前（最新）事件。
    pub fn last(&self) -> Option<&EvolutionEvent> {
        self.events.last()
    }

    /// 按事件类型统计数量。
    pub fn counts(&self) -> [u64; 4] {
        let mut c = [0u64; 4];
        for ev in &self.events {
            match ev.kind {
                EventKind::StateChange { .. } => c[0] += 1,
                EventKind::Compound { .. } => c[1] += 1,
                EventKind::Particle { .. } => c[2] += 1,
                EventKind::Crystallize { .. } => c[3] += 1,
            }
        }
        c
    }

    /// 从某步开始回放（返回该步及之后的事件序列；step 单调）。
    pub fn replay_from(&self, step: u64) -> impl Iterator<Item = &EvolutionEvent> {
        self.events.iter().filter(move |e| e.step >= step)
    }
}

/// 斐波那契：自然数的一种"进化模式"（演示：步进节奏的黄金比例演化）。
pub fn fibonacci_mode(limit: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let (mut a, mut b) = (0u64, 1u64);
    while a <= limit {
        out.push(a);
        let t = a + b;
        a = b;
        b = t;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interference::Particle;

    #[test]
    fn log_records_and_replays_in_order() {
        let mut log = EvolutionLog::new();
        log.record_state_change(State::Energy, State::Gas); // step 0
        log.tick();
        log.record_compound(0.42); // step 1
        log.tick();
        let p = Particle { layer: 1, position: 4, strength: 0.5, phase_diff: 1.9 };
        log.record_particle(&p); // step 2
        log.tick();
        log.record_crystallize(0.9); // step 3

        assert_eq!(log.step_now(), 3);
        assert_eq!(log.len(), 4);
        assert_eq!(log.last().unwrap().step, 3);
        let c = log.counts();
        assert_eq!(c, [1, 1, 1, 1]);

        let replay: Vec<u64> = log.replay_from(1).map(|e| e.step).collect();
        assert_eq!(replay, vec![1, 2, 3]);
    }

    #[test]
    fn state_change_ignores_same_state() {
        let mut log = EvolutionLog::new();
        log.record_state_change(State::Solid, State::Solid);
        assert_eq!(log.len(), 0);
        log.record_state_change(State::Solid, State::Liquid);
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn cap_keeps_latest() {
        let mut log = EvolutionLog::new();
        for i in 0..(EvolutionLog::CAP + 50) {
            log.tick();
            log.record_compound(i as f32 / 100.0);
        }
        assert_eq!(log.len(), EvolutionLog::CAP);
        // 最早事件被挤出，最新仍在（step 从 51..1074 保留尾部，共 1024 条）
        assert_eq!(log.last().unwrap().step, EvolutionLog::CAP as u64 + 50);
    }

    #[test]
    fn fibonacci_is_natural_evolution_mode() {
        let f = fibonacci_mode(13);
        assert_eq!(f, vec![0, 1, 1, 2, 3, 5, 8, 13]);
    }
}
