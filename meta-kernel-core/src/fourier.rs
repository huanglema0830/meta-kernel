//! # 傅里叶变换与波动分析（Fourier）
//!
//! 将输入信号（时间域）分解为频率域，提取主导频率、频谱宽度、指定频点相位
//! （2.2）。频率、方向、节律、相位的源头是"0 锚点的扰动模式"——
//! 本模块使这些属性可**从输入序列中实时提取**，而非预设。
//! 所有相位/波形计算以 `π`（std 常量）为基础（2.3）。

use std::f32::consts::PI;

/// 频谱分析结果。
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    /// 幅度谱（len = n/2+1，含 DC 与 Nyquist）。
    pub magnitudes: Vec<f32>,
    /// 主导频率 bin（不含 DC，除非无交流成分）。
    pub dominant_bin: usize,
    /// 主导频率（周期/采样，0..0.5）。
    pub dominant_freq: f32,
    /// 频谱宽度（以主导 bin 为中心、功率加权标准差，0..0.5）。
    pub width: f32,
    /// 总能量（RMS²）。
    pub energy: f32,
}

/// 实数序列 DFT（naive，O(n²)；窗口 ≤256 足够）。
pub fn dft(samples: &[f32]) -> Spectrum {
    let n = samples.len();
    let half = n / 2;
    let mut magnitudes = vec![0.0f32; half + 1];
    let mut energy = 0.0f32;
    if n < 2 {
        return Spectrum { magnitudes: vec![0.0], dominant_bin: 0, dominant_freq: 0.0, width: 0.0, energy: 0.0 };
    }
    for k in 0..=half {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for (i, s) in samples.iter().enumerate() {
            let angle = 2.0 * PI * k as f32 * i as f32 / n as f32;
            re += s * angle.cos();
            im -= s * angle.sin();
        }
        magnitudes[k] = (re * re + im * im).sqrt() / n as f32;
    }
    energy = samples.iter().map(|s| s * s).sum::<f32>() / n as f32;

    // 主导 bin：跳过 DC（除非全频能量都集中在 DC，即平直信号）
    let mut dominant_bin = 0usize;
    let mut best = magnitudes[0];
    for k in 1..=half {
        if magnitudes[k] > best {
            best = magnitudes[k];
            dominant_bin = k;
        }
    }
    let dominant_freq = if dominant_bin == 0 { 0.0 } else { dominant_bin as f32 / n as f32 };

    // 宽度：功率加权标准差（围绕主导 bin）
    let pow: Vec<f32> = magnitudes.iter().map(|m| m * m).collect();
    let psum: f32 = pow.iter().sum::<f32>().max(1e-12);
    let mut mean_k = 0.0f32;
    for (k, p) in pow.iter().enumerate() {
        mean_k += k as f32 * p / psum;
    }
    let mut var = 0.0f32;
    for (k, p) in pow.iter().enumerate() {
        var += (k as f32 - mean_k) * (k as f32 - mean_k) * p / psum;
    }
    let width = (var.sqrt() / n as f32).min(0.5);

    Spectrum { magnitudes, dominant_bin, dominant_freq, width, energy }
}

/// 指定频点（bin）的相位（rad，(-π, π]）。
pub fn phase_at(samples: &[f32], bin: usize) -> f32 {
    let n = samples.len();
    if n < 2 || bin > n / 2 {
        return 0.0;
    }
    let mut re = 0.0f32;
    let mut im = 0.0f32;
    for (i, s) in samples.iter().enumerate() {
        let angle = 2.0 * PI * bin as f32 * i as f32 / n as f32;
        re += s * angle.cos();
        im -= s * angle.sin();
    }
    im.atan2(re)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(freq_bin: usize, n: usize, phase: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * freq_bin as f32 * i as f32 / n as f32 + phase).sin())
            .collect()
    }

    #[test]
    fn sine_dominant_frequency_detected() {
        let n = 64;
        let x = sine(8, n, 0.0);
        let s = dft(&x);
        assert_eq!(s.dominant_bin, 8);
        assert!((s.dominant_freq - 8.0 / 64.0).abs() < 1e-6);
        assert!(s.width < 0.01, "单频窄谱: {}", s.width);
        assert!(s.energy > 0.4, "正弦能量: {}", s.energy);
    }

    #[test]
    fn phase_at_matches_known_shift() {
        // DFT(e^{-jθ}) 口径下，sin(θ+p) 的相位 = p - π/2（正交偏移为常数，
        // 差值与相位累积仍正确；interference 中该偏移自然抵消）
        let n = 64;
        let p = 1.0f32;
        let x = sine(4, n, p);
        let phi = phase_at(&x, 4);
        let expected = p - PI / 2.0;
        let d = ((phi - expected).abs())
            .min((phi - expected + 2.0 * PI).abs())
            .min((phi - expected - 2.0 * PI).abs());
        assert!(d < 1e-2, "phase {phi} vs expected {expected}");
        // 相位差（两列波）与真实差一致（偏移抵消）
        let y = sine(4, n, 0.0);
        let d2 = (phase_at(&x, 4) - phase_at(&y, 4) - p).abs();
        assert!(d2 < 1e-2, "relative phase error {d2}");
    }

    #[test]
    fn flat_signal_dc_only() {
        let n = 32;
        let x = vec![0.5f32; n];
        let s = dft(&x);
        assert_eq!(s.dominant_bin, 0);
        assert_eq!(s.dominant_freq, 0.0);
    }

    #[test]
    fn noise_has_wide_spectrum() {
        let n = 64;
        let x: Vec<f32> = (0..n).map(|i| ((i * 37 % n) as f32 / n as f32 - 0.5) * 2.0).collect();
        let s = dft(&x);
        assert!(s.width > 0.05, "噪声谱宽: {}", s.width);
    }
}
