//! L6 · **脸切换**：原版模式 / 场域模式。
//!
//! 依据：发起人方向修正（v0.111）P1-5「脸切换（L6）：原版模式/场域模式」。
//!
//! ## 语义（关键：**不是二选一的替换**）
//! | 模式 | 页面内容 | 场域叠层 |
//! |---|---|---|
//! | **原版 Original** | 照常显示 | **不叠加** |
//! | **场域 Field** | **照常显示**（用户看到熟悉的画面） | 叠加（按置信度决定强度） |
//! | **混合 Blend** | 照常显示 | 叠加，强度减半 |
//!
//! **设计约束**：无论哪种模式，**网页本身始终可见**——本模块只决定"叠不叠、叠多浓"，
//! 不做"把网页替换成另一套画面"的事。这直接对应验收标准里的两条同时成立：
//! 「能看网页、用户看到熟悉的画面」＋「原版模式/场域模式可切换」。

use crate::l1_mapping::VisualParams;

/// 脸模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceMode {
    /// 原版（不叠加）。
    Original,
    /// 场域（叠加，强度随置信度）。
    Field,
    /// 混合（叠加，强度减半）。
    Blend,
}

impl Default for FaceMode {
    fn default() -> Self {
        FaceMode::Original
    }
}

impl FaceMode {
    /// 稳定键（UI/localStorage 用；**非自由文本**）。
    pub fn key(&self) -> &'static str {
        match self {
            FaceMode::Original => "original",
            FaceMode::Field => "field",
            FaceMode::Blend => "blend",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            FaceMode::Original => "原版",
            FaceMode::Field => "场域",
            FaceMode::Blend => "混合",
        }
    }
    /// 解析（未知输入 → 原版，**安全缺省：不叠加**）。
    pub fn parse(s: &str) -> FaceMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "field" | "场域" => FaceMode::Field,
            "blend" | "混合" => FaceMode::Blend,
            _ => FaceMode::Original,
        }
    }
    /// 全部模式（UI 依次渲染）。
    pub fn all() -> [FaceMode; 3] {
        [FaceMode::Original, FaceMode::Field, FaceMode::Blend]
    }

    /// **叠层强度**（0 = 不叠；上限 0.35，**永不遮住内容**）。
    /// - 原版 → 0
    /// - 混合 → 场域的一半
    /// - 场域 → 随置信度线性（0.15 起，避免"看不清"时乱叠）
    pub fn overlay_alpha(&self, confidence: f64) -> f64 {
        let c = confidence.clamp(0.0, 1.0);
        let base = match self {
            FaceMode::Original => return 0.0,
            FaceMode::Blend => (0.15 + 0.85 * c) * 0.5,
            FaceMode::Field => 0.15 + 0.85 * c,
        };
        (base * 0.35).clamp(0.0, 0.35)
    }

    /// 是否叠加。
    pub fn overlays(&self) -> bool {
        !matches!(self, FaceMode::Original)
    }
}

/// 叠层输出（给宿主/UI 用的**纯值**；不产生 HTML）。
#[derive(Clone, Debug, PartialEq)]
pub struct FaceOverlay {
    pub mode: FaceMode,
    /// 叠层强度（0 → 调用方应完全不渲染叠层）。
    pub alpha: f64,
    /// 画面参数（原版模式仍返回，便于"切回原版时平滑过渡"）。
    pub params: VisualParams,
    /// CSS 变量串。
    pub css: String,
}

/// 生成叠层描述。
pub fn overlay(mode: FaceMode, params: &VisualParams, confidence: f64, css: String) -> FaceOverlay {
    FaceOverlay { mode, alpha: mode.overlay_alpha(confidence), params: *params, css }
}

/// 是否应该实际渲染叠层（强度太低时不渲染，省性能）。
pub fn should_render(o: &FaceOverlay) -> bool {
    o.alpha > 0.005
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l1_mapping::{render, seed_into};
    use crate::l1_field_parse::{parse, PageSignal};
    use crate::gene_library::GeneLibrary;

    fn params_and_conf() -> (VisualParams, f64) {
        let mut l = GeneLibrary::new();
        seed_into(&mut l);
        let r = parse(&PageSignal {
            text_len: 3000, paragraph_count: 12, heading_count: 4, link_count: 8,
            image_count: 3, media_count: 1, interactive_count: 2, script_count: 2,
        });
        (render(&r, &l), r.confidence)
    }

    #[test]
    fn original_never_overlays() {
        assert!(!FaceMode::Original.overlays());
        assert_eq!(FaceMode::Original.overlay_alpha(1.0), 0.0);
        assert_eq!(FaceMode::Original.overlay_alpha(0.0), 0.0);
    }

    #[test]
    fn field_overlays_and_blend_is_half() {
        let f = FaceMode::Field.overlay_alpha(1.0);
        let b = FaceMode::Blend.overlay_alpha(1.0);
        assert!(f > 0.0 && b > 0.0);
        assert!((f - b * 2.0).abs() < 1e-9, "混合＝场域的一半: {f} vs {b}");
    }

    #[test]
    fn alpha_is_capped_so_content_stays_visible() {
        for m in FaceMode::all() {
            for c in [0.0, 0.5, 1.0, 5.0, -1.0] {
                let a = m.overlay_alpha(c);
                assert!((0.0..=0.35).contains(&a), "{m:?} c={c} → {a} 越界");
            }
        }
        assert!(FaceMode::Field.overlay_alpha(1.0) <= 0.35, "**永不遮住网页内容**");
    }

    #[test]
    fn alpha_grows_with_confidence() {
        assert!(FaceMode::Field.overlay_alpha(0.9) > FaceMode::Field.overlay_alpha(0.1));
    }

    #[test]
    fn parse_is_total_and_safe_by_default() {
        assert_eq!(FaceMode::parse("field"), FaceMode::Field);
        assert_eq!(FaceMode::parse("场域"), FaceMode::Field);
        assert_eq!(FaceMode::parse("blend"), FaceMode::Blend);
        assert_eq!(FaceMode::parse("Original"), FaceMode::Original);
        // 未知/恶意输入 → 原版（不叠加，最安全）
        assert_eq!(FaceMode::parse("<script>"), FaceMode::Original);
        assert_eq!(FaceMode::parse(""), FaceMode::Original);
        assert_eq!(FaceMode::parse("');alert(1)//"), FaceMode::Original);
        assert_eq!(FaceMode::default(), FaceMode::Original);
    }

    #[test]
    fn keys_and_labels_are_stable() {
        assert_eq!(FaceMode::all().len(), 3);
        for m in FaceMode::all() {
            assert!(!m.label().is_empty());
            assert_eq!(FaceMode::parse(m.key()), m, "key 往返一致");
        }
    }

    #[test]
    fn overlay_respects_should_render() {
        let (p, c) = params_and_conf();
        let css = crate::l1_mapping::to_css(&p);
        let o_orig = overlay(FaceMode::Original, &p, c, css.clone());
        assert!(!should_render(&o_orig), "原版不渲染叠层");
        let o_field = overlay(FaceMode::Field, &p, c, css);
        assert!(should_render(&o_field), "场域渲染叠层");
        assert_eq!(o_field.mode, FaceMode::Field);
        assert!(o_field.css.contains("--fld-hue:"));
    }
}
