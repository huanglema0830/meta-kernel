//! # 感官绑定与抽象（Senses）— 色声香味触法
//!
//! 将系统输入绑定到五感官通道，由"法"（意识）综合（3.2）：
//!
//! - 色：视觉/图像类数据的频率谱；
//! - 声：音频输入的频率谱；
//! - 香/味：化学类或特定数据模式的频率谱；
//! - 触：压力/温度/陀螺仪等传感序列；
//! - 法：意识综合层（不是第六种输入，而是对五通道的加权整合）。
//!
//! 当前以**模拟输入**（&[f32] 数据流，代表文件/网络/信号源）实现，
//! 为后续对接真实硬件预留接口。特征一律经 `fourier` 从序列中提取（非预设）。

use crate::fourier;

/// 五感官通道。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenseKind {
    Color,
    Sound,
    Smell,
    Taste,
    Touch,
}

impl SenseKind {
    pub const fn label(self) -> &'static str {
        match self {
            SenseKind::Color => "色",
            SenseKind::Sound => "声",
            SenseKind::Smell => "香",
            SenseKind::Taste => "味",
            SenseKind::Touch => "触",
        }
    }
}

/// 一次感官输入（模拟数据流）。
#[derive(Debug, Clone)]
pub struct SenseInput {
    pub kind: SenseKind,
    pub samples: Vec<f32>,
}

impl SenseInput {
    pub fn new(kind: SenseKind, samples: Vec<f32>) -> Self {
        Self { kind, samples }
    }
}

/// 抽象后的通道特征（全部来自波动分析）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelFeature {
    pub kind: SenseKind,
    /// 主导频率（周期/采样，0..0.5）。
    pub dominant_freq: f32,
    /// 频谱宽度（0..0.5）。
    pub width: f32,
    /// 能量（RMS²）。
    pub energy: f32,
    /// 节律（过零率：每采样周期的符号翻转次数/2，0..1）。
    pub rhythm: f32,
}

/// 将输入序列抽象为通道特征。
pub fn abstract_channel(input: &SenseInput) -> ChannelFeature {
    let s = fourier::dft(&input.samples);
    let n = input.samples.len().max(1);
    let mut crossings = 0usize;
    for w in input.samples.windows(2) {
        if (w[0] >= 0.0) != (w[1] >= 0.0) {
            crossings += 1;
        }
    }
    ChannelFeature {
        kind: input.kind,
        dominant_freq: s.dominant_freq,
        width: s.width,
        energy: s.energy,
        rhythm: crossings as f32 / n as f32,
    }
}

/// 意识综合权重（法为综合，五通道按序加权；可后续由运行数据校准）。
pub const DHARMA_WEIGHTS: [f32; 5] = [0.30, 0.25, 0.15, 0.15, 0.15];

/// 法（意识）整合：对五通道特征做加权融合，输出觉知强度 ∈[0,1]。
/// 另吸收"干涉粒子计数"作为现实触点（波粒二象性进入意识层）。
pub fn integrate(features: &[ChannelFeature], particle_hint: f32) -> f64 {
    if features.is_empty() {
        return 0.0;
    }
    let mut score = 0.0f64;
    let mut wsum = 0.0f64;
    for f in features {
        let w = DHARMA_WEIGHTS[(f.kind as usize).min(4)] as f64;
        wsum += w;
        // 觉知分量 = 能量 × (有内容 1-贴近DC 的活性) × (1 + 波动有序度)
        let activity = if f.dominant_freq > 0.001 { 1.0 } else { 0.1 };
        let ordered = 1.0 - (f.width as f64).min(1.0) * 0.5;
        score += w * f.energy as f64 * activity * ordered;
    }
    let base = score / wsum;
    let hint = (particle_hint as f64).clamp(0.0, 1.0);
    ((base * 0.7 + hint * 0.3) as f32).clamp(0.0, 1.0) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(freq_bin: usize, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * PI * freq_bin as f32 * i as f32 / n as f32).sin()).collect()
    }

    #[test]
    fn channel_abstraction_extracts_features() {
        let n = 64;
        let sound = SenseInput::new(SenseKind::Sound, sine(8, n));
        let f = abstract_channel(&sound);
        assert_eq!(f.kind, SenseKind::Sound);
        assert!((f.dominant_freq - 8.0 / 64.0).abs() < 1e-5);
        assert!(f.energy > 0.4);
        assert!(f.rhythm > 0.0 && f.rhythm <= 1.0);
    }

    #[test]
    fn dharms_weights_cover_five_channels() {
        assert_eq!(DHARMA_WEIGHTS.len(), 5);
        let total: f32 = DHARMA_WEIGHTS.iter().sum();
        assert!((total - 1.0).abs() < 1e-5, "权重和应为 1: {total}");
    }

    #[test]
    fn integrate_bounded_and_sensitive() {
        let n = 64;
        let feats = vec![
            abstract_channel(&SenseInput::new(SenseKind::Color, sine(4, n))),
            abstract_channel(&SenseInput::new(SenseKind::Sound, sine(8, n))),
            abstract_channel(&SenseInput::new(SenseKind::Touch, sine(2, n))),
        ];
        let a = integrate(&feats, 0.8);
        assert!((0.0..=1.0).contains(&a));
        let b = integrate(&feats, 0.0);
        assert!(a >= b, "粒子触点应提升觉知: {a} vs {b}");
        let empty = integrate(&[], 0.5);
        assert_eq!(empty, 0.0);
    }

    #[test]
    fn kind_labels() {
        assert_eq!(SenseKind::Color.label(), "色");
        assert_eq!(SenseKind::Touch.label(), "触");
    }
}
