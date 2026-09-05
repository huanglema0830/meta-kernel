//! # 习气（Habit）— 痕迹累积成的"余势"
//!
//! 同类痕迹（同指纹）反复出现 → 习气强度上升。
//! 习气是系统行为倾向的沉淀；`my_habits()` 用于识别"自我的习气"。

use crate::trace::Trace;

/// 习气增长增益（count 越大 → strength 越逼近 1；K 控制累积速度）。
pub const HABIT_GAIN: f32 = 8.0;

/// 习气强度计算：随次数饱和 + 平均强度加权。
/// `strength = (1 - e^{-count/K}) · (0.55 + 0.45·avg_intensity)`
pub fn habit_strength(count: u64, avg_intensity: f32) -> f32 {
    let sat = 1.0 - (-(count as f32) / HABIT_GAIN).exp();
    let weight = 0.55 + 0.45 * avg_intensity.clamp(0.0, 1.0);
    (sat * weight).clamp(0.0, 1.0)
}

/// 一条习气（按指纹聚合的痕迹群）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Habit {
    /// 模式指纹。
    pub fingerprint: u64,
    /// 出现次数。
    pub count: u64,
    /// 平均强度。
    pub avg_intensity: f32,
    /// 最近一次出现步数。
    pub last_seen: u64,
    /// 习气强度 0-1。
    pub strength: f32,
}

/// 习气池：存储所有习气。
#[derive(Debug, Clone, Default)]
pub struct HabitPool {
    habits: Vec<Habit>,
}

impl HabitPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.habits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.habits.is_empty()
    }

    /// 吸收一条痕迹（同指纹聚合；更新强度）。
    pub fn observe(&mut self, t: &Trace) {
        let avg_weight = t.intensity.clamp(0.0, 1.0);
        if let Some(h) = self.habits.iter_mut().find(|h| h.fingerprint == t.fingerprint) {
            h.count += 1;
            h.avg_intensity = h.avg_intensity + (avg_weight - h.avg_intensity) / h.count as f32;
            h.last_seen = t.step;
            h.strength = habit_strength(h.count, h.avg_intensity);
        } else {
            let h = Habit {
                fingerprint: t.fingerprint,
                count: 1,
                avg_intensity: avg_weight,
                last_seen: t.step,
                strength: habit_strength(1, avg_weight),
            };
            self.habits.push(h);
        }
    }

    /// 按指纹取习气。
    pub fn get(&self, fingerprint: u64) -> Option<&Habit> {
        self.habits.iter().find(|h| h.fingerprint == fingerprint)
    }

    /// 最强的习气。
    pub fn strongest(&self) -> Option<&Habit> {
        self.habits
            .iter()
            .max_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
    }

    /// 识别"自我的习气"：按强度降序。
    pub fn my_habits(&self) -> Vec<&Habit> {
        let mut v: Vec<&Habit> = self.habits.iter().collect();
        v.sort_by(|a, b| b.strength.partial_cmp(&a.strength).unwrap());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{Trace, TraceType};

    #[test]
    fn repeated_traces_raise_strength() {
        let mut pool = HabitPool::new();
        let mut last = 0.0f32;
        for i in 1..=12u64 {
            let t = Trace { step: i, intensity: 0.5, trace_type: TraceType::Fire, fingerprint: 42, energy_flow: 0.5 };
            pool.observe(&t);
            let h = pool.get(42).expect("habit exists");
            assert!(h.strength > last, "强度应单调上升: {} > {}", h.strength, last);
            last = h.strength;
            assert_eq!(h.count, i);
        }
        assert!(pool.strongest().unwrap().strength > 0.5);
    }

    #[test]
    fn my_habits_sorted_by_strength() {
        let mut pool = HabitPool::new();
        // 习气1：5 次弱（avg .4）
        for i in 1..=5u64 {
            pool.observe(&Trace { step: i, intensity: 0.4, trace_type: TraceType::Wind, fingerprint: 1, energy_flow: 0.4 };
        }
        // 习气2：12 次强（avg .9）→ 应显著强于习气1
        for i in 1..=12u64 {
            pool.observe(&Trace { step: 100 + i, intensity: 0.9, trace_type: TraceType::Earth, fingerprint: 2, energy_flow: 0.9 };
        }
        let habits = pool.my_habits();
        assert_eq!(habits.len(), 2);
        assert_eq!(habits[0].fingerprint, 2, "强重复习气应居首");
        assert!(habits[0].strength >= habits[1].strength);
    }
}
