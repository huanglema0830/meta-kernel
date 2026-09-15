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

    /// 全部习气（插入序），供落盘前检查与往返比对。
    pub fn all(&self) -> &[Habit] {
        &self.habits
    }
    /// 序列化：每行一条习气（**无表头**；空池 → 空串）。
    ///
    /// 零依赖纯文本编解码——**序列化在内核，文件 IO 在宿主**。
    pub fn to_text(&self) -> String {
        let mut s = String::with_capacity(self.habits.len() * 48);
        for h in &self.habits {
            s.push_str(&h.to_text());
            s.push('\n');
        }
        s
    }
    /// 反序列化：忽略空行与非法行（不重放——习气是**沉淀后的状态**，直接恢复）。
    pub fn from_text(text: &str) -> Self {
        let mut v: Vec<Habit> = Vec::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            if let Some(h) = Habit::from_text(l) {
                v.push(h);
            }
        }
        Self { habits: v }
    }
}

impl Habit {
    /// 单行文本：`fingerprint|count|avg_intensity|last_seen|strength`。
    pub fn to_text(&self) -> String {
        format!(
            "{}|{}|{:.6}|{}|{:.6}",
            self.fingerprint, self.count, self.avg_intensity, self.last_seen, self.strength
        )
    }
    /// 反序列化（字段数不符 / 数值非法 → `None`）。
    pub fn from_text(line: &str) -> Option<Self> {
        let p: Vec<&str> = line.split('|').collect();
        if p.len() != 5 {
            return None;
        }
        let fingerprint: u64 = p[0].trim().parse().ok()?;
        let count: u64 = p[1].trim().parse().ok()?;
        let avg_intensity: f32 = p[2].trim().parse().ok()?;
        let last_seen: u64 = p[3].trim().parse().ok()?;
        let strength: f32 = p[4].trim().parse().ok()?;
        if !avg_intensity.is_finite() || !strength.is_finite() {
            return None;
        }
        Some(Habit { fingerprint, count, avg_intensity, last_seen, strength })
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
            pool.observe(&Trace { step: i, intensity: 0.4, trace_type: TraceType::Wind, fingerprint: 1, energy_flow: 0.4 });
        }
        // 习气2：12 次强（avg .9）→ 应显著强于习气1
        for i in 1..=12u64 {
            pool.observe(&Trace { step: 100 + i, intensity: 0.9, trace_type: TraceType::Earth, fingerprint: 2, energy_flow: 0.9 });
        }
        let habits = pool.my_habits();
        assert_eq!(habits.len(), 2);
        assert_eq!(habits[0].fingerprint, 2, "强重复习气应居首");
        assert!(habits[0].strength >= habits[1].strength);
    }

    /// 浮点字段保留 6 位小数 → 按 **1e-6 容差**比对；整数字段逐位比。
    fn assert_habit_eq(a: &Habit, b: &Habit) {
        assert_eq!(a.fingerprint, b.fingerprint, "指纹逐位一致");
        assert_eq!(a.count, b.count, "次数逐位一致");
        assert_eq!(a.last_seen, b.last_seen, "last_seen 逐位一致");
        assert!((a.avg_intensity - b.avg_intensity).abs() < 1e-6, "avg {} vs {}", a.avg_intensity, b.avg_intensity);
        assert!((a.strength - b.strength).abs() < 1e-6, "strength {} vs {}", a.strength, b.strength);
    }

    #[test]
    fn habit_text_roundtrip_preserves_fields() {
        let h = Habit { fingerprint: 4242, count: 9, avg_intensity: 0.63, last_seen: 120, strength: habit_strength(9, 0.63) };
        let back = Habit::from_text(&h.to_text()).expect("自编码必须可解码");
        assert_habit_eq(&back, &h);
    }

    #[test]
    fn habit_from_text_rejects_malformed() {
        assert!(Habit::from_text("").is_none());
        assert!(Habit::from_text("1|2|3").is_none(), "字段数不符");
        assert!(Habit::from_text("x|2|0.5|3|0.4").is_none(), "指纹非法");
        assert!(Habit::from_text("1|2|NaN|3|0.4").is_none(), "NaN 拒绝");
    }

    #[test]
    fn pool_text_roundtrip_restores_habits() {
        let mut pool = HabitPool::new();
        for i in 1..=5u64 {
            pool.observe(&Trace { step: i, intensity: 0.4, trace_type: TraceType::Wind, fingerprint: 1, energy_flow: 0.4 });
        }
        for i in 1..=12u64 {
            pool.observe(&Trace { step: 100 + i, intensity: 0.9, trace_type: TraceType::Earth, fingerprint: 2, energy_flow: 0.9 });
        }
        let back = HabitPool::from_text(&pool.to_text());
        assert_eq!(back.len(), pool.len(), "习气条数一致");
        for (a, b) in pool.all().iter().zip(back.all().iter()) {
            assert_habit_eq(a, b);
        }
        // 真实语义：**强度排序关系**也被恢复（强重复习气仍居首）
        assert_eq!(back.strongest().unwrap().fingerprint, 2, "重启后最强习气仍是指纹 2");
    }

    #[test]
    fn pool_from_text_ignores_bad_lines_and_empty() {
        let mut pool = HabitPool::new();
        pool.observe(&Trace { step: 1, intensity: 0.5, trace_type: TraceType::Fire, fingerprint: 77, energy_flow: 0.5 });
        let dirty = format!("oops\n\n{}\n1|2|3\n", pool.to_text());
        let back = HabitPool::from_text(&dirty);
        assert_eq!(back.len(), 1, "非法行被忽略");
        assert_eq!(back.all()[0].fingerprint, 77);
        assert!(HabitPool::from_text("").is_empty(), "空文本 → 空池");
    }
}
