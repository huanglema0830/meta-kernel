//! L1–L3 · **场域解析器**：从「内容」到「场域状态」的转换。
//!
//! 依据：发起人方向修正（v0.111）——空天浏览器 = **能看网页的浏览器 + 场域呈现能力**；
//! 逐层补齐 P0-1「场域解析器（L1–L3）：从内容到场域状态的转换」。
//!
//! ## 分层与职责
//! - **L1（抽取）**：由**宿主**完成（DOM/渲染信息 → [`PageSignal`]）。内核**不碰 DOM**，
//!   只接收已经抽取好的**结构化信号**（这样内核保持零依赖、可测、跨平台）。
//! - **L2（归一）**：把原始计数归一为 0..1 的强度（饱和曲线，抑制长尾）。
//! - **L3（定格）**：按**固定且可解释的公式**把强度映射到四场（地/水/火/风）。
//!
//! ## 四场语义（与 L5 一致，不自创新词）
//! | 场 | 含义 | 本解析器的信号来源 |
//! |---|---|---|
//! | **地 earth** | 结构 / 稳定 | 标题与段落的结构度（标题/段落比） |
//! | **水 water** | 流动 / 连续性 | 正文文本量（信息连续体） |
//! | **火 fire** | 强度 / 刺激性 | 媒体与图片密度（视觉刺激） |
//! | **风 wind** | 变化 / 交互性 | 链接与可交互控件密度 |
//!
//! **不做伪能力宣称**：本模块只做「信号 → 状态」的**确定性换算**，不做"理解网页语义"的宣称；
//! 情绪/意图等更高层判断属 L5，不在此层。

/// 页面信号（**宿主抽取**；内核只读）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageSignal {
    /// 可见文本字符数。
    pub text_len: u32,
    /// 段落数。
    pub paragraph_count: u32,
    /// 标题数（h1–h6）。
    pub heading_count: u32,
    /// 链接数。
    pub link_count: u32,
    /// 图片数。
    pub image_count: u32,
    /// 媒体元素数（video/audio/canvas）。
    pub media_count: u32,
    /// 可交互控件数（input/button/select/textarea）。
    pub interactive_count: u32,
    /// 脚本数（间接反映动态性）。
    pub script_count: u32,
}

/// 内容类别（粗分类；用于选场景，不用于判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentClass {
    /// 以正文为主（文章/文档）。
    Article,
    /// 以媒体为主（图/视频）。
    Media,
    /// 以交易与操作为主（表单/按钮密集）。
    Commerce,
    /// 以社交与链接流为主。
    Social,
    /// 工具型（控件多、正文少）。
    Tool,
    /// 信号不足，无法分类。
    Unknown,
}

impl ContentClass {
    pub fn label(&self) -> &'static str {
        match self {
            ContentClass::Article => "文章/文档",
            ContentClass::Media => "媒体",
            ContentClass::Commerce => "交易/表单",
            ContentClass::Social => "社交/链接流",
            ContentClass::Tool => "工具",
            ContentClass::Unknown => "未知",
        }
    }
    pub fn code(&self) -> u8 {
        match self {
            ContentClass::Article => 1,
            ContentClass::Media => 2,
            ContentClass::Commerce => 3,
            ContentClass::Social => 4,
            ContentClass::Tool => 5,
            ContentClass::Unknown => 0,
        }
    }
}

/// 场域读数（四场 + 置信度）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldReading {
    pub earth: f64,
    pub water: f64,
    pub fire: f64,
    pub wind: f64,
    /// 置信度 0..1（信号量越足越可信）。
    pub confidence: f64,
}

impl FieldReading {
    pub fn to_array(&self) -> [f64; 4] {
        [self.earth, self.water, self.fire, self.wind]
    }
    pub fn dominant(&self) -> (usize, f64) {
        let a = self.to_array();
        let mut bi = 0;
        for i in 1..4 {
            if a[i] > a[bi] {
                bi = i;
            }
        }
        (bi, a[bi])
    }
    pub fn dominant_label(&self) -> &'static str {
        ["地", "水", "火", "风"][self.dominant().0]
    }
}

/// **L2 归一**：饱和曲线 `x/(x+k)` → 0..1（抑制长尾，避免"网页越长越极端"）。
/// `k` = 半饱和点（达到 k 时取 0.5）。
pub fn normalize(x: f64, k: f64) -> f64 {
    let kk = if k <= 0.0 { 1.0 } else { k };
    let v = if x < 0.0 { 0.0 } else { x };
    (v / (v + kk)).clamp(0.0, 1.0)
}

/// 半饱和常数（**有依据的取值**，写死以便复现；改动需记录在版本说明）。
pub const K_TEXT_CHARS: f64 = 2000.0;
pub const K_PARAGRAPHS: f64 = 12.0;
pub const K_HEADINGS: f64 = 4.0;
pub const K_LINKS: f64 = 40.0;
pub const K_IMAGES: f64 = 10.0;
pub const K_MEDIA: f64 = 2.0;
pub const K_INTERACTIVE: f64 = 20.0;

/// 结构度：标题相对段落的密度（结构清晰 → 高）。
pub fn structure_ratio(s: &PageSignal) -> f64 {
    if s.paragraph_count == 0 {
        return 0.0;
    }
    (s.heading_count as f64 / s.paragraph_count as f64).min(1.0)
}

/// **L3 定格**：信号 → 四场。
///
/// 公式（全部可解释、可复现）：
/// - `earth = 0.6·structure + 0.4·norm(paragraphs)`（结构清楚、段落成型 → 地稳）
/// - `water = norm(text_len)`（信息连续体 → 水）
/// - `fire  = 0.6·norm(images) + 0.4·norm(media)`（视觉刺激 → 火）
/// - `wind  = 0.6·norm(links) + 0.4·norm(interactive)`（变化与交互 → 风）
pub fn parse(s: &PageSignal) -> FieldReading {
    let structure = structure_ratio(s);
    let earth = 0.6 * structure + 0.4 * normalize(s.paragraph_count as f64, K_PARAGRAPHS);
    let water = normalize(s.text_len as f64, K_TEXT_CHARS);
    let fire = 0.6 * normalize(s.image_count as f64, K_IMAGES)
        + 0.4 * normalize(s.media_count as f64, K_MEDIA);
    let wind = 0.6 * normalize(s.link_count as f64, K_LINKS)
        + 0.4 * normalize(s.interactive_count as f64, K_INTERACTIVE);
    let confidence = confidence_of(s);
    FieldReading { earth, water, fire, wind, confidence }
}

/// 置信度：文本量为主、结构为辅（信号太稀少 → 不敢下结论）。
/// 注意：**先转 f64 再相加**——u32 直接相加在极端输入下会溢出 panic（v0.111 由极端输入测试抓出）。
pub fn confidence_of(s: &PageSignal) -> f64 {
    let text = normalize(s.text_len as f64, 500.0);
    let struct_ = normalize(s.paragraph_count as f64 + s.heading_count as f64, 8.0);
    (0.7 * text + 0.3 * struct_).clamp(0.0, 1.0)
}

/// 粗分类（阈值写死；用于选场景，不参与判定）。
pub fn classify(s: &PageSignal) -> ContentClass {
    if s.text_len < 50 && s.paragraph_count == 0 {
        return ContentClass::Unknown;
    }
    let text = s.text_len as f64;
    // 同样：**先转 f64 再相乘相加**（避免 u32 溢出 panic）
    let media = s.image_count as f64 + s.media_count as f64 * 3.0;
    let inter = s.interactive_count as f64;
    let links = s.link_count as f64;
    if inter >= 8.0 && text < 800.0 {
        return ContentClass::Tool;
    }
    if inter >= 4.0 && text < 3000.0 {
        return ContentClass::Commerce;
    }
    if links >= 20.0 && text < 1200.0 {
        return ContentClass::Social;
    }
    if media >= 6.0 && media > text / 400.0 {
        return ContentClass::Media;
    }
    if text >= 600.0 {
        return ContentClass::Article;
    }
    ContentClass::Unknown
}

// ===== v0.112 接线：场域指纹 → 痕迹池 =====
//
// 复用**已有**的痕迹设施（不自造第二套）：
// - `trace::fingerprint_of`（能量流模式指纹）→ 同模式聚合成**习气**
// - `trace::decide_type`（风水火地类型）→ 痕迹分类
// - `trace::Trace` 可直接交给 `TraceStore::record` 或 `HabitPool::observe`

/// 四场读数的波幅（离散度），供痕迹类型判定用。
pub fn volatility_of(r: &FieldReading) -> f32 {
    let a = r.to_array();
    let mean = (a[0] + a[1] + a[2] + a[3]) / 4.0;
    let mut v = 0.0;
    for x in a {
        v += (x - mean) * (x - mean);
    }
    ((v / 4.0).sqrt() as f32).clamp(0.0, 1.0)
}

/// 四场均值（作为"能量流"口径）。
pub fn flow_of(r: &FieldReading) -> f32 {
    let a = r.to_array();
    (((a[0] + a[1] + a[2] + a[3]) / 4.0).clamp(0.0, 1.0)) as f32
}

/// **场域指纹**（接入痕迹池）：同场域模式 → 同指纹 → 习气累积。
pub fn field_fingerprint(r: &FieldReading) -> u64 {
    let s = [r.earth as f32, r.water as f32, r.fire as f32, r.wind as f32];
    crate::trace::fingerprint_of(&s, flow_of(r))
}

/// 痕迹类型（复用 `trace::decide_type`；不自创分类）。
pub fn field_trace_type(r: &FieldReading) -> crate::trace::TraceType {
    let a = r.to_array();
    let volatility = volatility_of(r);
    let compound = (a[0] + a[2]) as f32 / 2.0; // 地+火 视为"复合活动"
    crate::trace::decide_type(volatility, compound, flow_of(r))
}

/// **生成一条痕迹**（可直接 `TraceStore::record` / `HabitPool::observe`）。
pub fn field_trace(r: &FieldReading, step: u64) -> crate::trace::Trace {
    crate::trace::Trace {
        step,
        intensity: r.confidence.clamp(0.0, 1.0) as f32,
        trace_type: field_trace_type(r),
        fingerprint: field_fingerprint(r),
        energy_flow: flow_of(r),
    }
}

/// 七维签名（接入 `dna_trace` 七维痕迹池：`match_trace` / `store`）。
pub fn field_signature7(r: &FieldReading) -> [f64; 7] {
    [r.earth, r.water, r.fire, r.wind, r.confidence, flow_of(r) as f64, volatility_of(r) as f64]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article() -> PageSignal {        PageSignal {
            text_len: 6000, paragraph_count: 20, heading_count: 6, link_count: 5,
            image_count: 1, media_count: 0, interactive_count: 0, script_count: 3,
        }
    }
    fn media() -> PageSignal {
        PageSignal {
            text_len: 300, paragraph_count: 2, heading_count: 1, link_count: 10,
            image_count: 40, media_count: 5, interactive_count: 2, script_count: 20,
        }
    }
    fn tool() -> PageSignal {
        PageSignal {
            text_len: 200, paragraph_count: 1, heading_count: 1, link_count: 3,
            image_count: 0, media_count: 0, interactive_count: 30, script_count: 30,
        }
    }

    #[test]
    fn normalize_is_saturating_and_bounded() {
        assert!((normalize(0.0, 10.0) - 0.0).abs() < 1e-12);
        assert!((normalize(10.0, 10.0) - 0.5).abs() < 1e-12, "半饱和点 = k");
        assert!(normalize(1e9, 10.0) < 1.0, "永不越界");
        assert!(normalize(-5.0, 10.0) == 0.0, "负值归零");
        assert!((normalize(5.0, 0.0) - 5.0 / 6.0).abs() < 1e-12, "k=0 → 退化为 1 保护");
    }

    #[test]
    fn parse_is_bounded_for_any_signal() {
        let extreme = PageSignal {
            text_len: u32::MAX, paragraph_count: u32::MAX, heading_count: u32::MAX,
            link_count: u32::MAX, image_count: u32::MAX, media_count: u32::MAX,
            interactive_count: u32::MAX, script_count: u32::MAX,
        };
        let r = parse(&extreme);
        for v in r.to_array() {
            assert!((0.0..=1.0).contains(&v), "越界: {v}");
        }
        let r0 = parse(&PageSignal::default());
        for v in r0.to_array() {
            assert!((0.0..=1.0).contains(&v));
        }
        assert!(r0.confidence < 0.3, "空信号置信度低");
    }

    #[test]
    fn article_leans_earth_water() {
        let r = parse(&article());
        assert!(r.earth > 0.4, "结构清楚 → 地不弱: {}", r.earth);
        assert!(r.water > 0.5, "正文量大 → 水强: {}", r.water);
        assert!(r.fire < 0.3, "媒体少 → 火弱: {}", r.fire);
        assert_eq!(classify(&article()), ContentClass::Article);
    }

    #[test]
    fn media_leans_fire() {
        let r = parse(&media());
        assert!(r.fire > 0.5, "媒体密集 → 火强: {}", r.fire);
        assert_eq!(classify(&media()), ContentClass::Media, "媒体分类");
    }

    #[test]
    fn tool_is_interactive_and_tool_class() {
        let r = parse(&tool());
        // 风的公式以链接为主权重（0.6），工具页链接少但控件多 → 风为中等，**但明显高于纯文章页**
        assert!(r.wind > 0.2, "交互控件密集 → 风不弱: {}", r.wind);
        assert!(r.wind > parse(&article()).wind, "工具页的风应强于文章页");
        assert_eq!(classify(&tool()), ContentClass::Tool);
    }

    /// **极端输入不得 panic**（u32 溢出在 debug 下会 panic —— v0.111 曾因此失败一次）
    #[test]
    fn extreme_counts_do_not_overflow() {
        for sig in [
            PageSignal { text_len: u32::MAX, paragraph_count: u32::MAX, heading_count: u32::MAX, ..Default::default() },
            PageSignal { image_count: u32::MAX, media_count: u32::MAX, ..Default::default() },
            PageSignal { link_count: u32::MAX, interactive_count: u32::MAX, ..Default::default() },
            PageSignal { media_count: u32::MAX / 2, ..Default::default() },
        ] {
            let r = parse(&sig);
            assert!((0.0..=1.0).contains(&r.confidence));
            let _ = classify(&sig);
        }
    }

    #[test]
    fn dominant_and_labels() {
        assert_eq!(parse(&media()).dominant_label(), "火");
        assert_eq!(parse(&article()).dominant_label(), "水");
        let (idx, v) = parse(&article()).dominant();
        assert!(idx < 4 && v > 0.0);
    }

    #[test]
    fn structure_ratio_handles_zero_paragraphs() {
        let s = PageSignal { paragraph_count: 0, heading_count: 5, ..Default::default() };
        assert_eq!(structure_ratio(&s), 0.0, "无段落不除零");
    }

    #[test]
    fn classification_covers_expected_shapes() {
        assert_eq!(classify(&PageSignal::default()), ContentClass::Unknown);
        let commerce = PageSignal { text_len: 800, paragraph_count: 5, heading_count: 2, interactive_count: 6, ..Default::default() };
        assert_eq!(classify(&commerce), ContentClass::Commerce);
        let social = PageSignal { text_len: 900, paragraph_count: 5, heading_count: 1, link_count: 30, ..Default::default() };
        assert_eq!(classify(&social), ContentClass::Social);
        for c in [ContentClass::Article, ContentClass::Unknown] {
            assert!(c.code() <= 5 && !c.label().is_empty());
        }
    }

    #[test]
    fn confidence_grows_with_signal() {
        let low = confidence_of(&PageSignal::default());
        let high = confidence_of(&article());
        assert!(high > low, "{high} 应大于 {low}");
        assert!(high <= 1.0);
    }

    // ===== v0.112 接线测试：场域指纹 → 痕迹池 =====

    #[test]
    fn fingerprint_is_stable_and_pattern_sensitive() {
        let a = parse(&article());
        let same = parse(&article());
        assert_eq!(field_fingerprint(&a), field_fingerprint(&same), "同模式 → 同指纹（幂等）");
        let m = parse(&media());
        assert_ne!(field_fingerprint(&a), field_fingerprint(&m), "不同模式 → 不同指纹");
    }

    #[test]
    fn trace_can_be_recorded_and_aggregated_into_habit() {
        use crate::habit::HabitPool;
        use crate::trace::TraceStore;
        let r = parse(&article());
        let mut store = TraceStore::with_cap(16);
        let mut pool = HabitPool::new();
        for step in 1..=5u64 {
            let t = field_trace(&r, step);
            assert!(t.intensity > 0.0 && t.energy_flow >= 0.0);
            pool.observe(&t);
            store.record(t);
        }
        assert_eq!(store.len(), 5, "痕迹入池");
        assert_eq!(pool.len(), 1, "同指纹聚合成 1 条习气");
    }

    #[test]
    fn signature7_feeds_dna_trace_pool() {
        use crate::dna_trace::{match_trace, store, AdaptKind};
        let r = parse(&article());
        let sig = field_signature7(&r);
        let mut lib = Vec::new();
        let id = store(&mut lib, sig, AdaptKind::FieldSense);
        assert!(id >= 1 && lib.len() == 1);
        assert_eq!(match_trace(&lib, &sig, 1e-9), Some(0), "同签名可命中");
        let other = field_signature7(&parse(&media()));
        assert!(match_trace(&lib, &other, 1e-9).is_none(), "异签名不误命中");
    }

    #[test]
    fn volatility_and_flow_are_bounded() {
        for r in [parse(&article()), parse(&media()), parse(&PageSignal::default())] {
            assert!((0.0..=1.0).contains(&volatility_of(&r)), "{}", volatility_of(&r));
            assert!((0.0..=1.0).contains(&flow_of(&r)));
            assert!(field_signature7(&r).iter().all(|x| (0.0..=1.0).contains(x)));
        }
    }
}
