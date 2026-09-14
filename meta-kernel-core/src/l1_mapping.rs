//! L1–L3 · **场域映射库**：从「场域状态」到「画面」的转换。
//!
//! 依据：发起人方向修正（v0.111）P1-6「场域映射库（L1–L3）：场域状态↔画面的映射公式」
//! 与「速度加强」表：**基因库 → 场域映射库存映射公式**、**学习机制 → 新映射自动入库**。
//!
//! ## 设计
//! - 每个画面参数 = 四场读数（地/水/火/风）的**加权合成**，权重即"映射公式"；
//! - 公式**存在基因库「基础公式层」**（复用已接线的 [`GeneLibrary::set_base_constant`] 同族能力，
//!   这里用 [`Formula::Weighted`]），命名 `fieldmap.<param>`；
//! - 基因库缺项 → **回退内置默认权重**（不破坏既有行为）；
//! - [`learn_mapping`] / [`nudge_weight`] 让新映射**自动入库**（学习机制的落点）。
//!
//! **不做伪能力宣称**：本模块只把"状态"换算成"画面参数"，**不宣称理解语义**；
//! 它给出的是**可解释、可复现、可学习**的映射，而不是"AI 生成的审美"。

use crate::gene_library::{Formula, GeneLibrary};
use crate::l1_field_parse::FieldReading;

/// 画面参数（全部 0..1 归一；由 [`to_css`] 映射到真实 CSS 值）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualParams {
    /// 色相（0 = 冷蓝，1 = 暖红）。
    pub hue: f64,
    /// 饱和度。
    pub sat: f64,
    /// 明度（越大越亮）。
    pub light: f64,
    /// 对比度。
    pub contrast: f64,
    /// 节奏（越大越快）。
    pub tempo: f64,
    /// 密度（越大越密）。
    pub density: f64,
    /// 圆润度。
    pub radius: f64,
}

impl Default for VisualParams {
    fn default() -> Self {
        Self { hue: 0.5, sat: 0.5, light: 0.5, contrast: 0.5, tempo: 0.5, density: 0.5, radius: 0.5 }
    }
}

/// 参数名（顺序与 [`VisualParams`] 字段一致）。
pub const PARAM_NAMES: [&str; 7] = ["hue", "sat", "light", "contrast", "tempo", "density", "radius"];

/// 基因库中的公式名（命名即契约；改名即改数据来源）。
pub const GENE_NAMES: [&str; 7] = [
    "fieldmap.hue",
    "fieldmap.sat",
    "fieldmap.light",
    "fieldmap.contrast",
    "fieldmap.tempo",
    "fieldmap.density",
    "fieldmap.radius",
];

/// 内置默认权重（四场顺序 = 地/水/火/风）。
///
/// 取值依据（可解释，非调参巧合）：
/// - **hue 火权重最大**：火（刺激/强度）推动画面转暖；
/// - **tempo 风权重最大**：风（变化/交互）推动节奏加快；
/// - **density 水权重最大**：水（信息连续体）推动排布变密；
/// - **light 受火抑制为 0.05**：火高时画面转浓（不是更亮）。
pub const DEFAULT_W: [[f64; 4]; 7] = [
    [0.10, 0.05, 0.85, 0.15], // hue
    [0.20, 0.30, 0.70, 0.30], // sat
    [0.35, 0.30, 0.05, 0.20], // light
    [0.30, 0.10, 0.55, 0.35], // contrast
    [0.05, 0.20, 0.45, 0.75], // tempo
    [0.35, 0.60, 0.40, 0.25], // density
    [0.30, 0.25, 0.10, 0.20], // radius
];

fn enc_w(w: &[f64; 4]) -> [f64; 7] {
    [w[0], w[1], w[2], w[3], 0.0, 0.0, 0.0]
}

/// **把默认映射公式登记进基因库**（幂等 upsert；返回登记条数）。
pub fn seed_into(lib: &mut GeneLibrary) -> usize {
    for i in 0..7 {
        let w = DEFAULT_W[i];
        lib.set_base_formula(
            GENE_NAMES[i],
            Formula::Weighted { dims: 4, w: enc_w(&w) },
            [1.0; 7],
        );
    }
    7
}

/// 读取某参数的映射权重（基因库优先；缺项/形态不符 → 回退默认）。
pub fn weights_of(lib: &GeneLibrary, i: usize) -> [f64; 4] {
    if i >= 7 {
        return [0.0; 4];
    }
    if let Some(g) = lib.base_formula(GENE_NAMES[i]) {
        if let Formula::Weighted { w, .. } = g {
            return [w[0], w[1], w[2], w[3]];
        }
    }
    DEFAULT_W[i]
}

/// **学习入库**：用新权重覆盖某参数的映射公式（"新映射自动入库"）。
pub fn learn_mapping(lib: &mut GeneLibrary, i: usize, w: [f64; 4]) -> Option<u32> {
    if i >= 7 {
        return None;
    }
    Some(lib.set_base_formula(GENE_NAMES[i], Formula::Weighted { dims: 4, w: enc_w(&w) }, [1.0; 7]))
}

/// **微调**：把某参数对某一场的权重朝 `delta` 方向调整（带边界，用于渐进积累）。
pub fn nudge_weight(lib: &mut GeneLibrary, i: usize, field: usize, delta: f64) -> Option<[f64; 4]> {
    if i >= 7 || field >= 4 {
        return None;
    }
    let mut w = weights_of(lib, i);
    w[field] = (w[field] + delta).clamp(0.0, 1.0);
    learn_mapping(lib, i, w);
    Some(w)
}

fn wsum(w: &[f64; 4], r: &FieldReading) -> f64 {
    let a = r.to_array();
    let mut s = 0.0;
    for i in 0..4 {
        s += w[i] * a[i];
    }
    s.clamp(0.0, 1.0)
}

/// **渲染**：场域读数 → 画面参数（权重取自基因库；极值一律夹到 0..1）。
pub fn render(r: &FieldReading, lib: &GeneLibrary) -> VisualParams {
    VisualParams {
        hue: wsum(&weights_of(lib, 0), r),
        sat: wsum(&weights_of(lib, 1), r),
        light: wsum(&weights_of(lib, 2), r),
        contrast: wsum(&weights_of(lib, 3), r),
        tempo: wsum(&weights_of(lib, 4), r),
        density: wsum(&weights_of(lib, 5), r),
        radius: wsum(&weights_of(lib, 6), r),
    }
}

/// 画面参数 → CSS 变量串（供 UI 直接套用；**不产生 HTML**，只产生键值）。
pub fn to_css(v: &VisualParams) -> String {
    let hue_deg = 200.0 + v.hue.clamp(0.0, 1.0) * 140.0;      // 200..340（冷蓝 → 暖红）
    let sat = 18.0 + v.sat.clamp(0.0, 1.0) * 62.0;            // 18..80 %
    let light = 82.0 - v.light.clamp(0.0, 1.0) * 44.0;        // 82..38 %
    let tempo_ms = (900.0 - v.tempo.clamp(0.0, 1.0) * 600.0).round();
    let gap = (6.0 + v.density.clamp(0.0, 1.0) * 10.0).round();
    let radius = (6.0 + v.radius.clamp(0.0, 1.0) * 12.0).round();
    let alpha = 0.06 + v.contrast.clamp(0.0, 1.0) * 0.18;     // 叠层不遮内容
    format!(
        "--fld-hue:{hue_deg:.0};--fld-sat:{sat:.0}%;--fld-light:{light:.0}%;--fld-tempo:{tempo_ms:.0}ms;--fld-gap:{gap:.0}px;--fld-radius:{radius:.0}px;--fld-alpha:{alpha:.3}",
    )
}

// ===== v0.112 接线：摩尼宝珠 → 场域呈现 =====
//
// 复用**已有**的摩尼宝珠常数与语义（不自造第二套）：
// - **镜面**：呈现层取"补相"，使叠层与内容互补而不重复（避免"内容与叠层同色 → 看不清"）
// - **闸门**：按黄金比例拆解（`gate::DECOMPOSE_RATIO` = 0.618…）——只取一部分，不一次吃满
// - **回归**：按 e^-0.1 向基线回落——扰动过后自动回稳

/// 闸门比例（黄金分割拆解；与 `gate` 模块同一常数）。
pub const GATE_RATIO: f64 = crate::gate::DECOMPOSE_RATIO;
/// 自然回归系数 e^-0.1（与能量回归同口径）。
pub const NATURAL_RETURN: f64 = 0.904_837_418_035_959_5;

/// **镜面（补相）**：`x → 1 - x`（对色相/密度/节奏取补，饱和度与明度保持正向）。
pub fn mirror_complement(v: &VisualParams) -> VisualParams {
    VisualParams {
        hue: (1.0 - v.hue).clamp(0.0, 1.0),
        sat: v.sat,
        light: (1.0 - v.light).clamp(0.0, 1.0),
        contrast: (1.0 - v.contrast).clamp(0.0, 1.0),
        tempo: v.tempo,
        density: (1.0 - v.density).clamp(0.0, 1.0),
        radius: v.radius,
    }
}

/// **闸门（×0.618 拆解）**：把强度按黄金比例收一档，避免"一次吃满"。
pub fn gate_once(v: &VisualParams) -> VisualParams {
    let g = |x: f64| (x * GATE_RATIO).clamp(0.0, 1.0);
    VisualParams {
        hue: g(v.hue), sat: g(v.sat), light: g(v.light), contrast: g(v.contrast),
        tempo: g(v.tempo), density: g(v.density), radius: g(v.radius),
    }
}

/// **回归（×e^-0.1 向基线）**。
pub fn regress_to(v: &VisualParams, base: &VisualParams) -> VisualParams {
    let r = |x: f64, b: f64| (b + (x - b) * NATURAL_RETURN).clamp(0.0, 1.0);
    VisualParams {
        hue: r(v.hue, base.hue), sat: r(v.sat, base.sat), light: r(v.light, base.light),
        contrast: r(v.contrast, base.contrast), tempo: r(v.tempo, base.tempo),
        density: r(v.density, base.density), radius: r(v.radius, base.radius),
    }
}

/// 摩尼宝珠链路留痕（**镜面 → 闸门 → 回归**三步的结果与中间量）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PearlTrace {
    pub mirrored: VisualParams,
    pub gated: VisualParams,
    pub regressed: VisualParams,
}

/// **摩尼宝珠 → 场域呈现**：镜面 → 闸门 → 回归，返回三步结果（最终用 `regressed`）。
pub fn through_pearl(v: &VisualParams, base: &VisualParams) -> PearlTrace {
    let mirrored = mirror_complement(v);
    let gated = gate_once(&mirrored);
    let regressed = regress_to(&gated, base);
    PearlTrace { mirrored, gated, regressed }
}

// ===== v0.112 接线：场域状态 → 基因库「场景公式层」=====

/// 由页面信号定出**场景标识**（同一"内容类别 + 四场量化桶" → 同一场景）。
///
/// 量化到 8 档后再哈希，使"相似内容"共享场景条目（避免场景爆炸）。
pub fn field_scene_id(sig: &crate::l1_field_parse::PageSignal, r: &FieldReading) -> u32 {
    let q = |x: f64| (x.clamp(0.0, 1.0) * 7.0).round() as u32; // 0..7
    let class = crate::l1_field_parse::classify(sig).code() as u32;
    let key = format!("fsc:{}:{}:{}:{}:{}", class, q(r.earth), q(r.water), q(r.fire), q(r.wind));
    (crate::gene_library::fnv1a64(0xcbf2_9ce4_8422_2325, key.as_bytes()) % 1_000_000) as u32
}

/// **把场域状态写入基因库场景公式层**（同场景复用；未命中则建立并登记）。
/// 返回 `(场景 id, 是否命中已有本底)`。
pub fn learn_field_scene(
    lib: &mut GeneLibrary,
    sig: &crate::l1_field_parse::PageSignal,
    r: &FieldReading,
) -> (u32, bool) {
    use crate::l5_baseline::BaselineField;
    let sid = field_scene_id(sig, r);
    if lib.scene_of(sid).is_some() {
        return (sid, true);
    }
    let b = BaselineField {
        earth: r.earth,
        water: r.water,
        fire: r.fire,
        wind: r.wind,
        object: "page",
        established: "learned",
    };
    let params = [r.confidence, crate::l1_field_parse::flow_of(r) as f64,
        crate::l1_field_parse::volatility_of(r) as f64, 0.0];
    lib.learn_scene(sid, "page", b, params);
    (sid, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l1_field_parse::{parse, PageSignal};

    fn lib() -> GeneLibrary {
        let mut l = GeneLibrary::new();
        seed_into(&mut l);
        l
    }
    fn fire_heavy() -> FieldReading {
        parse(&PageSignal {
            text_len: 300, paragraph_count: 2, heading_count: 1, link_count: 10,
            image_count: 40, media_count: 5, interactive_count: 2, script_count: 20,
        })
    }
    fn wind_heavy() -> FieldReading {
        parse(&PageSignal {
            text_len: 200, paragraph_count: 1, heading_count: 1, link_count: 60,
            image_count: 0, media_count: 0, interactive_count: 40, script_count: 5,
        })
    }

    #[test]
    fn seed_is_idempotent_and_readable() {
        let mut l = GeneLibrary::new();
        assert_eq!(seed_into(&mut l), 7);
        seed_into(&mut l);
        assert_eq!(l.base.len(), 7, "重复 seed 不新增");
        for i in 0..7 {
            assert_eq!(weights_of(&l, i), DEFAULT_W[i], "第 {i} 条与默认一致");
        }
    }

    #[test]
    fn missing_gene_falls_back_to_defaults() {
        let empty = GeneLibrary::new();
        for i in 0..7 {
            assert_eq!(weights_of(&empty, i), DEFAULT_W[i], "空库回退默认");
        }
        assert_eq!(weights_of(&empty, 99), [0.0; 4], "越界索引安全");
    }

    #[test]
    fn render_is_bounded() {
        let l = lib();
        for r in [fire_heavy(), wind_heavy(), FieldReading { earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0, confidence: 1.0 }] {
            let v = render(&r, &l);
            for x in [v.hue, v.sat, v.light, v.contrast, v.tempo, v.density, v.radius] {
                assert!((0.0..=1.0).contains(&x), "越界 {x}");
            }
        }
    }

    /// **语义可解释性**：火强 → 更暖、节奏与密度的差异符合设计意图。
    #[test]
    fn fire_raises_hue_and_wind_raises_tempo() {
        let l = lib();
        let calm = parse(&PageSignal { text_len: 4000, paragraph_count: 15, heading_count: 4, ..Default::default() });
        let hot = fire_heavy();
        let windy = wind_heavy();
        assert!(render(&hot, &l).hue > render(&calm, &l).hue, "火强 → 更暖");
        assert!(render(&windy, &l).tempo > render(&calm, &l).tempo, "风强 → 节奏更快");
    }

    /// **验收：新映射自动入库**（学习机制落点）。
    #[test]
    fn learned_mapping_takes_effect_and_is_stored() {
        let mut l = lib();
        let r = fire_heavy();
        let before = render(&r, &l).hue;
        // 把 hue 完全交给"地"（人为反向映射）
        learn_mapping(&mut l, 0, [1.0, 0.0, 0.0, 0.0]).unwrap();
        let after = render(&r, &l).hue;
        assert_ne!(before, after, "学习后画面变化");
        assert_eq!(weights_of(&l, 0), [1.0, 0.0, 0.0, 0.0], "公式已入库");
        assert!(l.verify_chain() || l.chain.is_empty(), "入库不破坏链");
    }

    #[test]
    fn nudge_is_bounded_and_persisted() {
        let mut l = lib();
        let w0 = weights_of(&l, 4);
        let w1 = nudge_weight(&mut l, 4, 3, 0.1).unwrap();
        assert!((w1[3] - (w0[3] + 0.1).min(1.0)).abs() < 1e-12);
        assert_eq!(weights_of(&l, 4), w1, "微调已入库");
        let w2 = nudge_weight(&mut l, 4, 3, 5.0).unwrap();
        assert!(w2[3] <= 1.0, "上界保护");
        assert!(nudge_weight(&mut l, 9, 0, 0.1).is_none(), "越界索引不 panic");
    }

    #[test]
    fn css_output_is_value_only_and_complete() {
        let l = lib();
        let css = to_css(&render(&fire_heavy(), &l));
        for key in ["--fld-hue:", "--fld-sat:", "--fld-light:", "--fld-tempo:", "--fld-gap:", "--fld-radius:", "--fld-alpha:"] {
            assert!(css.contains(key), "缺 {key}: {css}");
        }
        assert!(!css.contains('<') && !css.contains('>'), "只出键值，不含标签");
        assert!(css.contains('%') && css.contains("ms"), "单位齐备");
    }

    #[test]
    fn param_name_table_matches_gene_names() {
        assert_eq!(PARAM_NAMES.len(), 7);
        assert_eq!(GENE_NAMES.len(), 7);
        assert!(GENE_NAMES.iter().all(|n| n.starts_with("fieldmap.")));
    }

    // ===== v0.112 接线测试：摩尼宝珠 → 场域呈现 =====

    #[test]
    fn pearl_three_steps_are_math_correct() {
        let base = VisualParams::default();
        let v = VisualParams { hue: 0.9, sat: 0.8, light: 0.7, contrast: 0.6, tempo: 0.5, density: 0.4, radius: 0.3 };
        let t = through_pearl(&v, &base);
        assert!((t.mirrored.hue - 0.1).abs() < 1e-12, "镜面取补相");
        assert!((t.mirrored.sat - v.sat).abs() < 1e-12, "饱和度不补");
        assert!((t.gated.hue - 0.1 * GATE_RATIO).abs() < 1e-12, "闸门 ×0.618");
        // 回归：向基线（0.5）靠近
        assert!((t.regressed.hue - (0.5 + (t.gated.hue - 0.5) * NATURAL_RETURN)).abs() < 1e-12);
        assert!((t.regressed.hue - 0.5).abs() < (t.gated.hue - 0.5).abs(), "回归靠近基线");
    }

    #[test]
    fn pearl_chain_stays_bounded_and_gate_never_inflates() {
        let base = VisualParams::default();
        for v in [
            VisualParams::default(),
            VisualParams { hue: 1.0, sat: 1.0, light: 1.0, contrast: 1.0, tempo: 1.0, density: 1.0, radius: 1.0 },
            VisualParams { hue: 0.0, sat: 0.0, light: 0.0, contrast: 0.0, tempo: 0.0, density: 0.0, radius: 0.0 },
        ] {
            let t = through_pearl(&v, &base);
            for x in [t.mirrored, t.gated, t.regressed] {
                for y in [x.hue, x.sat, x.light, x.contrast, x.tempo, x.density, x.radius] {
                    assert!((0.0..=1.0).contains(&y), "越界 {y}");
                }
            }
            // 闸门只会收缩（≤ 原值），不会放大
            assert!(t.gated.hue <= t.mirrored.hue + 1e-12 || t.mirrored.hue == 0.0);
        }
        assert!(GATE_RATIO > 0.6 && GATE_RATIO < 0.62, "闸门比例即黄金分割: {GATE_RATIO}");
    }

    #[test]
    fn regress_long_run_returns_to_baseline() {
        let base = VisualParams::default();
        let mut v = VisualParams { hue: 1.0, sat: 1.0, light: 1.0, contrast: 1.0, tempo: 1.0, density: 1.0, radius: 1.0 };
        for _ in 0..300 {
            v = regress_to(&v, &base);
        }
        assert!((v.hue - base.hue).abs() < 1e-6 && (v.tempo - base.tempo).abs() < 1e-6, "长期回归基线");
    }

    // ===== v0.112 接线测试：场域状态 → 场景公式层 =====

    #[test]
    fn field_scene_is_stable_and_reuses() {
        let mut l = GeneLibrary::new();
        let sig = PageSignal { text_len: 4000, paragraph_count: 15, heading_count: 4, link_count: 4, ..Default::default() };
        let r = parse(&sig);
        let id1 = field_scene_id(&sig, &r);
        assert_eq!(id1, field_scene_id(&sig, &r), "同信号 → 同场景 id");
        let (sid, hit1) = learn_field_scene(&mut l, &sig, &r);
        assert!(!hit1, "首次建立");
        assert_eq!(sid, id1);
        assert_eq!(l.scene.len(), 1);
        let (sid2, hit2) = learn_field_scene(&mut l, &sig, &r);
        assert!(hit2 && sid2 == sid, "二次命中复用");
        assert_eq!(l.scene.len(), 1, "命中不新增条目");
        // 本底即该页四场
        let b = l.scene_of(sid).unwrap().base;
        assert!((b.earth - r.earth).abs() < 1e-12 && (b.fire - r.fire).abs() < 1e-12);
    }

    #[test]
    fn different_field_patterns_map_to_different_scenes() {
        let mut l = GeneLibrary::new();
        let a = PageSignal { text_len: 6000, paragraph_count: 20, heading_count: 6, ..Default::default() };
        let m = PageSignal { text_len: 300, paragraph_count: 2, heading_count: 1, image_count: 40, media_count: 5, ..Default::default() };
        let ra = parse(&a);
        let rm = parse(&m);
        let (s1, _) = learn_field_scene(&mut l, &a, &ra);
        let (s2, _) = learn_field_scene(&mut l, &m, &rm);
        assert_ne!(s1, s2, "不同场域模式 → 不同场景");
        assert_eq!(l.scene.len(), 2);
        assert!(l.verify_chain() || l.chain.is_empty());
    }
}
