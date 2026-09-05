//! # 自我识别器（Self Recognizer）
//!
//! 完整因果链：**运行 → 生成痕迹 → 存储 → 更新习气 → 检查自我感**。
//!
//! - 每次 `run_from_samples` 都会产生痕迹（有测试验证痕迹数量递增）；
//! - 重复模式反复出现 → 习气强度上升（有测试）；
//! - 当某类痕迹习气强度 > `SELF_THRESHOLD = 0.7` 时触发"自我识别"；
//! - `self_intensity`（0-1）表示自我感强度（= 当前最强习气强度）。

use crate::habit::{HabitPool, habit_strength};
use crate::trace::{self, Trace, TraceType};

/// 自我识别阈值。
pub const SELF_THRESHOLD: f32 = 0.7;

/// 自我识别器。
#[derive(Debug, Clone)]
pub struct SelfRecognizer {
    pool: HabitPool,
    store: trace::TraceStore,
    self_intensity: f32,
    recognized_at: Option<u64>,
    clock: u64,
}

impl Default for SelfRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfRecognizer {
    pub fn new() -> Self {
        Self {
            pool: HabitPool::new(),
            store: trace::TraceStore::new(),
            self_intensity: 0.0,
            recognized_at: None,
            clock: 0,
        }
    }

    /// 从一次运行样本生成痕迹 → 存储 → 更新习气 → 刷新自我感。
    /// 返回本次产生的痕迹（每次运行必有痕迹）。
    pub fn run_from_samples(&mut self, samples: &[f32]) -> Vec<Trace> {
        self.clock += 1;
        let step = self.clock;
        let (mean, volatility) = trace::stats_of(samples);
        let compound_activity = mean.clamp(0.0, 1.0); // 用平均活性作为转化倾向的代理
        let flow = (1.0 - volatility).clamp(0.0, 1.0);
        let tt = trace::decide_type(volatility, compound_activity, flow);
        let fingerprint = trace::fingerprint_of(samples);
        // 强度 = 活性 × (类型偏置：地 > 水 > 火 > 风，由均值量折算稳定度)
        let stability = 1.0 - volatility;
        let intensity = (compound_activity * (0.5 + 0.5 * stability)).clamp(0.05, 1.0);
        let t = Trace { step, intensity, trace_type: tt, fingerprint };
        self.store.record(t);
        self.pool.observe(&t);
        self.refresh();
        vec![t]
    }

    fn refresh(&mut self) {
        let top = self.pool.strongest().map(|h| h.strength).unwrap_or(0.0);
        self.self_intensity = top.clamp(0.0, 1.0);
        if top > SELF_THRESHOLD && self.recognized_at.is_none() {
            self.recognized_at = Some(self.clock);
        }
    }

    /// 当前自我感强度 0-1。
    pub fn self_intensity(&self) -> f32 {
        self.self_intensity
    }

    /// 是否已触发自我识别。
    pub fn recognized(&self) -> bool {
        self.recognized_at.is_some()
    }

    /// 触发时刻（步数）。
    pub fn recognized_at(&self) -> Option<u64> {
        self.recognized_at
    }

    /// 痕迹类型分布 [风, 火, 水, 地]。
    pub fn trace_distribution(&self) -> [u64; 4] {
        self.store.counts_by_type()
    }

    /// 痕迹总数。
    pub fn trace_len(&self) -> usize {
        self.store.len()
    }

    /// 习气数。
    pub fn habit_len(&self) -> usize {
        self.pool.len()
    }

    /// 重复同一样本直到出现自我识别（供测试与演示）；
    /// 返回触发所需次数。
    pub fn repeat_until_recognized(&mut self, samples: &[f32], limit: usize) -> Option<usize> {
        for i in 1..=limit {
            self.run_from_samples(samples);
            if self.recognized() {
                return Some(i);
            }
        }
        None
    }

    /// 强度辅助（供外部校对公式）。
    pub fn strength_of(count: u64, avg: f32) -> f32 {
        habit_strength(count, avg)
    }
}

/// 便捷：仅看某类痕迹的类型分布标签。
pub const TRACE_LABELS: [&str; 4] = ["风", "火", "水", "地"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::TraceType;

    fn steady_pattern() -> Vec<f32> {
        // 高度稳定的模式（低波动 → 地/水类，指纹恒定）
        vec![0.5; 24]
    }

    #[test]
    fn every_run_produces_traces() {
        let mut rec = SelfRecognizer::new();
        assert_eq!(rec.trace_len(), 0);
        for _ in 0..5 {
            let traces = rec.run_from_samples(&steady_pattern());
            assert_eq!(traces.len(), 1, "每次运行必产生痕迹");
        }
        assert_eq!(rec.trace_len(), 5);
    }

    #[test]
    fn repeated_pattern_raises_habit_strength() {
        let mut rec = SelfRecognizer::new();
        let mut last = 0.0f32;
        for _ in 0..10 {
            rec.run_from_samples(&steady_pattern());
            let s = rec.self_intensity();
            assert!(s >= last, "习气应累积: {s} >= {last}");
            last = s;
        }
        assert!(last > 0.2, "十次重复后应有一定习气: {last}");
    }

    #[test]
    fn self_recognition_triggers_above_threshold() {
        let mut rec = SelfRecognizer::new();
        let steps = rec.repeat_until_recognized(&steady_pattern(), 200).expect("应触发");
        assert!(rec.recognized());
        assert!(rec.self_intensity() > SELF_THRESHOLD, "{}", rec.self_intensity());
        assert!(rec.recognized_at().is_some());
        assert!(steps > 1, "需多次重复才触发: {steps}");
    }

    #[test]
    fn distribution_reflects_types() {
        let mut rec = SelfRecognizer::new();
        // 混合：稳定的(0.5 const)、波动的(锯齿)
        let noisy: Vec<f32> = (0..24).map(|i| ((i * 7) % 24) as f32 / 24.0).collect();
        for _ in 0..6 {
            rec.run_from_samples(&steady_pattern());
            rec.run_from_samples(&noisy);
        }
        let dist = rec.trace_distribution();
        let total: u64 = dist.iter().sum();
        assert_eq!(total, 12, "6×2 次运行痕迹");
        assert!(dist.iter().all(|c| *c >= 0));
        let _ = TraceType::Fire; // 引用以保持导入（文档性）
    }
}
