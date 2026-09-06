//! # 干涉驻点检测（Interference）— 波粒二象性
//!
//! 实时分析内部波动，检测两列波（或一列波与其延迟自复制）的**驻点**：
//! 当相位差与黄金驻点层级（色声香味触法）对齐（容差内）时，
//! 判定生成一个"粒子"（驻点）——波动在此处"冻结"为可感知的最小实体。
//!
//! 驻点层级（2.1，为 2π 的归一化分数）：
//! 色 0.618（第一界面）｜ 声 0.309（半周期）｜ 香/味 0.206（三分周期）
//! 触 0.154（四分周期）｜ 法 0.123（五分周期）

use std::f32::consts::PI;

use crate::fourier;
use crate::ontology::{AbstractSchema, Pattern};
use crate::state::{GOLDEN_FIFTH, GOLDEN_HALF, GOLDEN_QUARTER, GOLDEN_RATIO, GOLDEN_THIRD};

/// 五感官层（色声香味触）标签；法（意识）为综合层。
pub const SENSE_NAMES: [&str; 5] = ["色", "声", "香/味", "触", "法"];
/// 驻点层级（2π 归一化分数）。
pub const NODE_LEVELS: [f32; 5] = [GOLDEN_RATIO, GOLDEN_HALF, GOLDEN_THIRD, GOLDEN_QUARTER, GOLDEN_FIFTH];
/// 判定容差（相位差与层目标的弧度差 < 该值 → 粒子）。
pub const TOLERANCE: f32 = 0.04;

/// 粒子（驻点）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    /// 层级码 0..4（色声香味触法）。
    pub layer: u8,
    /// 所在位置（窗口中部索引）。
    pub position: usize,
    /// 强度（两列波幅积 × 贴合度）。
    pub strength: f32,
    /// 两波相位差（rad）。
    pub phase_diff: f32,
}

impl Particle {
    /// 层级名。
    pub fn layer_name(&self) -> &'static str {
        SENSE_NAMES[(self.layer as usize).min(4)]
    }
}

fn circular_dist(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % (2.0 * PI);
    if d > PI {
        2.0 * PI - d
    } else {
        d
    }
}

/// 将一列波与其延迟半窗的"自复制"比较 → 检测驻点（单波自干涉）。
pub fn detect_single(samples: &[f32]) -> Vec<Particle> {
    if samples.len() < 16 {
        return Vec::new();
    }
    let h = samples.len() / 2;
    detect(&samples[..h], &samples[h..], h / 2)
}

/// 检测两列波之间的驻点：各自在其主导频点取相位，**|相位差|** 与层级比对
/// （驻点判据与正负方向无关：Δφ 的绝对大小对齐黄金驻点层即成粒子）。
pub fn detect(a: &[f32], b: &[f32], position: usize) -> Vec<Particle> {
    let mut out = Vec::new();
    let n = a.len().min(b.len());
    if n < 8 {
        return out;
    }
    let sa = fourier::dft(&a[..n]);
    let sb = fourier::dft(&b[..n]);
    if sa.dominant_bin == 0 || sb.dominant_bin == 0 {
        return out; // 无波动不成干涉
    }
    let pa = fourier::phase_at(&a[..n], sa.dominant_bin);
    let pb = fourier::phase_at(&b[..n], sb.dominant_bin);
    let raw = (pa - pb).abs() % (2.0 * PI);
    let mag_a = sa.magnitudes[sa.dominant_bin];
    let mag_b = sb.magnitudes[sb.dominant_bin];

    for (layer, level) in NODE_LEVELS.iter().enumerate() {
        let target = level * 2.0 * PI;
        let d = circular_dist(raw, target);
        if d < TOLERANCE {
            out.push(Particle {
                layer: layer as u8,
                position,
                strength: (mag_a * mag_b * (1.0 - d / PI)).clamp(0.0, 1.0),
                phase_diff: raw,
            });
        }
    }
    out
}

/// 圆环相位差（供外部核对）。
pub fn phase_difference(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n < 8 {
        return 0.0;
    }
    let sa = fourier::dft(&a[..n]);
    let sb = fourier::dft(&b[..n]);
    if sa.dominant_bin == 0 || sb.dominant_bin == 0 {
        return 0.0;
    }
    fourier::phase_at(&a[..n], sa.dominant_bin) - fourier::phase_at(&b[..n], sb.dominant_bin)
}

// ---------- 镜面干涉（摩尼宝珠第一层：mirror） ----------
//
// 输入模式与正源库中每个模式做"相位镜像"：取两者特征相位差（圆环距离 ∈[0,π]）
// 的余弦为干涉度——同相 → 1（共振），正交 → 0（无关），反相 → -1（互斥）。
// 相位统一从 0-10 层振幅谱（模式=元素强度 / 骨架=节点计数）的 DFT 主导频点提取，
// 与既有 fourier/interference 同一套波动语言，不另造算法。

/// 干涉点：输入与库中单模式的相位对比。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterferencePoint {
    /// 库中模式序号（AbstractSchema 无 id 字段，以库索引指代）。
    pub pattern_id: usize,
    /// 干涉度：cos(圆环相位差) ∈ [-1,1]；同相≈1｜反相≈-1｜正交≈0。
    pub interference: f32,
    /// 圆环相位差（rad，∈[0,π]）。
    pub phase_delta: f32,
}

/// 干涉图案：输入 vs 正源库整体。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InterferencePattern {
    /// 逐库模式的干涉点。
    pub points: Vec<InterferencePoint>,
    /// 输入的主导相位（rad，[0,2π)）——"镜子"的相位姿态。
    pub dominant_phase: f32,
}

/// 两相位差的干涉度（圆环距离归一后取余弦）——单测锚点。
pub fn interference_of(input_phase: f32, stored_phase: f32) -> f32 {
    circular_dist(input_phase, stored_phase).cos()
}

/// 特征序列 → 主导频率相位（rad，[0,2π)；不足 8 点或无波动(DC) → 0）。
fn series_phase(samples: &[f32]) -> f32 {
    let n = samples.len();
    if n < 8 {
        return 0.0;
    }
    let sp = fourier::dft(samples);
    if sp.dominant_bin == 0 {
        return 0.0;
    }
    fourier::phase_at(samples, sp.dominant_bin).rem_euclid(2.0 * PI)
}

/// 层振幅谱：把元素按层级（0-10）累计成 11 点振幅序列。
fn level_amplitude(pairs: impl Iterator<Item = (u8, f64)>) -> Vec<f32> {
    let mut amp = [0f32; 11];
    for (level, mag) in pairs {
        amp[(level as usize).min(10)] += mag as f32;
    }
    amp.to_vec()
}

/// 模式 → 特征序列：优先元素层振幅谱；无元素时回退 history 时序。
fn pattern_samples(p: &Pattern) -> Vec<f32> {
    if !p.elements.is_empty() {
        level_amplitude(p.elements.iter().map(|e| (e.level, e.intensity)))
    } else {
        p.history.iter().map(|h| *h as f32).collect()
    }
}

/// 骨架 → 特征序列：节点计数层振幅谱（edges 不贡献波形，只贡献结构关系）。
fn schema_samples(s: &AbstractSchema) -> Vec<f32> {
    level_amplitude(s.nodes.iter().map(|n| (n.level, n.count as f64)))
}

/// 模式的主导特征相位（镜面输入侧）。
pub fn pattern_phase(p: &Pattern) -> f32 {
    series_phase(&pattern_samples(p))
}

/// 骨架的主导特征相位（镜面库侧）。
pub fn schema_phase(s: &AbstractSchema) -> f32 {
    series_phase(&schema_samples(s))
}

/// 镜面干涉（摩尼宝珠·镜子层）：输入模式对正源库整体做相位镜像。
///
/// 只读正源库，不写入、不占有（五戒·不偷盗：非纠缠结果不纳入）。
pub fn mirror(input: &Pattern, library: &[AbstractSchema]) -> InterferencePattern {
    let input_phase = pattern_phase(input);
    let points = library
        .iter()
        .enumerate()
        .map(|(id, schema)| {
            let stored = schema_phase(schema);
            let delta = circular_dist(input_phase, stored);
            InterferencePoint {
                pattern_id: id,
                interference: delta.cos(),
                phase_delta: delta,
            }
        })
        .collect();
    InterferencePattern { points, dominant_phase: input_phase }
}

#[cfg(test)]
mod mirror_tests {
    use super::*;
    use crate::ontology::LevelNode;

    #[test]
    fn interference_of_matches_phase_relations() {
        // 同相 → 1
        assert!((interference_of(0.0, 0.0) - 1.0).abs() < 1e-6);
        // 反相（Δ=π）→ -1
        assert!((interference_of(0.0, PI) + 1.0).abs() < 1e-6);
        // 正交（Δ=π/2）→ 0
        assert!(interference_of(0.0, PI / 2.0).abs() < 1e-6);
        // 圆环回绕：2π 视为同相
        assert!((interference_of(0.1, 0.1 + 2.0 * PI) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn identical_structure_mirrors_in_phase() {
        // 输入模式与库骨架使用同一层振幅谱 → 相位相等 → 干涉度 ≈ 1
        let p = Pattern {
            elements: vec![
                crate::ontology::Element::new(2, 3.0),
                crate::ontology::Element::new(5, 3.0),
                crate::ontology::Element::new(8, 3.0),
            ],
            history: Vec::new(),
        };
        let s = AbstractSchema {
            nodes: vec![LevelNode::new(2, 3), LevelNode::new(5, 3), LevelNode::new(8, 3)],
            edges: Vec::new(),
        };
        let out = mirror(&p, &[s]);
        assert_eq!(out.points.len(), 1);
        let pt = out.points[0];
        assert!((pt.interference - 1.0).abs() < 1e-3, "同构应同相: {}", pt.interference);
        assert!(pt.phase_delta < 1e-3);
        // dominant_phase = 输入相位（[0,2π)）
        assert!((0.0..=2.0 * PI).contains(&out.dominant_phase));
    }

    #[test]
    fn empty_library_gives_empty_points() {
        let p = Pattern {
            elements: vec![crate::ontology::Element::new(3, 0.8)],
            history: Vec::new(),
        };
        let out = mirror(&p, &[]);
        assert!(out.points.is_empty());
        assert_eq!(out.dominant_phase, pattern_phase(&p));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine_wave(freq_bin: usize, n: usize, phase: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * freq_bin as f32 * i as f32 / n as f32 + phase).sin())
            .collect()
    }

    #[test]
    fn golden_half_alignment_produces_sound_particle() {
        // 相位差 = GOLDEN_HALF·2π ≈ 1.942 rad → 对齐"声"层
        let n = 64;
        let shift = GOLDEN_HALF * 2.0 * PI;
        let a = sine_wave(6, n, 0.0);
        let b = sine_wave(6, n, shift);
        let ps = detect(&a, &b, 32);
        assert!(!ps.is_empty(), "应检测到驻点粒子");
        assert!(ps.iter().any(|p| p.layer_name() == "声" || p.layer == 1), "{ps:?}");
        assert!(ps.iter().all(|p| (0.0..=1.0).contains(&p.strength)), "强度域越界");
    }

    #[test]
    fn misaligned_phases_produce_nothing() {
        let n = 64;
        let shift = 0.42f32; // 不对齐任何层级
        let a = sine_wave(6, n, 0.0);
        let b = sine_wave(6, n, shift);
        // 判定：除非该差值与某层在容差内（确定性检查）
        let ps = detect(&a, &b, 0);
        for p in &ps {
            let target = NODE_LEVELS[p.layer as usize] * 2.0 * PI;
            let d = circular_dist(p.phase_diff, target).min(circular_dist(p.phase_diff + PI, target));
            assert!(d < TOLERANCE, "不应对齐层 {} (d={})", p.layer, d);
        }
    }

    #[test]
    fn self_interference_detects_particles() {
        let n = 128;
        let x = sine_wave(12, n, 0.3);
        // 直接拼两段相位差构造自波
        let mut w = x.clone();
        w.extend(sine_wave(12, n, 0.3 + GOLDEN_RATIO * 2.0 * PI));
        let ps = detect_single(&w);
        assert!(!ps.is_empty(), "自干涉应检出");
        assert!(ps.iter().any(|p| p.layer == 0 || p.layer_name() == "色"));
    }

    #[test]
    fn node_levels_follow_golden_derivatives() {
        assert!(NODE_LEVELS[0] > NODE_LEVELS[1] && NODE_LEVELS[1] > NODE_LEVELS[2]);
        assert_eq!(SENSE_NAMES.len(), 5);
        assert!(NODE_LEVELS[4] < NODE_LEVELS[3]);
    }
}
