//! L1–L3 · **场域解析器 · 源码直解版**（不依赖渲染、不依赖 DOM、不依赖宿主抽取）
//!
//! 依据：专家源头设计（2026-09-15）「场域解析器必须能直接从 HTML/CSS 源码提取场域状态，
//! 不依赖任何渲染后的 DOM」；以及"WebView2 是不可依赖的外部组件"这一根本定位。
//!
//! ## 为什么必须源码直解
//! 旧路径 `PageSignal` 要宿主（WebView2）先抽取信号 —— 一旦脱离 WebView2，输入源就没了。
//! 本模块**只看源码文本**：给一段 HTML/CSS，就能算出四场，**不渲染、不建 DOM、不联网**。
//!
//! ## 零依赖实现（与项目红线的关系）
//! 专家建议用 `html5ever` / `lightningcss`。但**"内核零依赖"是本项目质量红线**，
//! 故此处**自研轻量扫描器**：只做**结构统计**（标签计数/嵌套深度/兄弟数/文本量），
//! 等价于"code-only 特征提取"的**必要子集**，不追求完整解析 HTML 语义。
//! 代价：不处理极端畸形 HTML 的完整规范语义；收益：**零依赖、可测、可跑在任意平台**。
//!
//! ## 四场定义（沿用既有语义，不自创新词）
//! - **地 earth**：结构度 = 嵌套深度 × 兄弟节点数 × 标签多样性
//! - **水 water**：正文量 = 文本字符数 / 元素总数
//! - **火 fire** ：媒体密度 = (img/video/audio/canvas/svg/picture) / 元素总数（+CSS 动效加权）
//! - **风 wind**：交互密度 = (a/button/input/select/textarea/form) / 元素总数（+CSS 布局/媒体查询加权）
//!
//! 归一化统一用**饱和曲线** `saturate(x,k) = 1 - e^(-k·x)`（专家给定），
//! 保证任意输入都落在 `[0,1)` 且单调、无界输入不发散。

use crate::l1_field_parse::{ContentClass, FieldReading};

/// 饱和曲线（专家给定形式）：`1 - e^(-k·x)`，`x ≥ 0`。
pub fn saturate(x: f64, k: f64) -> f64 {
    // NaN → 0；+INF 走同一公式自然得到 1（exp(-INF)=0）
    let xx = if x.is_nan() { 0.0 } else if x > 0.0 { x } else { 0.0 };
    let kk = if k.is_finite() && k > 0.0 { k } else { 0.0 };
    (1.0 - (-kk * xx).exp()).clamp(0.0, 1.0)
}

/// 空元素（无闭合标签），不计入嵌套。
pub const VOID_TAGS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// 媒体元素。
pub const MEDIA_TAGS: [&str; 7] = ["img", "video", "audio", "canvas", "svg", "picture", "source"];

/// 交互元素。
pub const INTERACTIVE_TAGS: [&str; 6] = ["a", "button", "input", "select", "textarea", "form"];

/// HTML 源码统计（**只统计，不建树**）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceStats {
    /// 元素总数。
    pub elements: u32,
    /// 非空白文本字符数。
    pub text_chars: u32,
    /// 最大嵌套深度。
    pub max_depth: u32,
    /// 父节点平均子元素数。
    pub avg_children: f64,
    /// 单节点最大子元素数（衡量"扇出"：列表/画廊/导航的宽度）。
    pub max_children: u32,
    /// 不同标签种数。
    pub unique_tags: u32,
    /// 媒体元素数。
    pub media: u32,
    /// 交互元素数。
    pub interactive: u32,
    /// 链接数（a[href]）。
    pub links: u32,
    /// script 元素数。
    pub scripts: u32,
    /// style 元素数。
    pub styles: u32,
    /// HTML 注释数。
    pub comments: u32,
}

/// CSS 源码统计。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssStats {
    /// 规则块数（`{` 计数）。
    pub rules: u32,
    /// 媒体查询数（`@media`）。
    pub media_queries: u32,
    /// 布局声明数（flex/grid/columns）。
    pub layout_decls: u32,
    /// 动效声明数（animation/transition/transform）。
    pub motion_decls: u32,
    /// 颜色声明数（color/background/background-color/fill/stroke）。
    pub color_decls: u32,
    /// 选择器总数（`,` 与 `{` 的粗略计）。
    pub selectors: u32,
}

/// 小写化并取标签名（遇到空白 / `>` / `/` 停止）。
fn tag_name_of(raw: &str) -> String {
    let mut s = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == ':' {
            s.push(ch.to_ascii_lowercase());
        } else {
            break;
        }
    }
    s
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// **扫描 HTML 源码**（单遍、无递归、无第三方依赖）。
///
/// 健壮性：未闭合标签、深层嵌套、二进制垃圾、超长输入都不 panic（逐字节处理 + 深度只计数）。
pub fn scan_html(html: &str) -> SourceStats {
    let b = html.as_bytes();
    let mut st = SourceStats::default();
    let mut tags: Vec<String> = Vec::new();
    let mut unique: Vec<String> = Vec::new();
    let mut max_depth: u32 = 0;
    // 每层的子元素计数栈（真正的层级统计）
    let mut child_stack: Vec<u32> = Vec::new();
    let mut child_counts: Vec<u32> = Vec::new();
    let mut in_script = false;
    let mut in_style = false;
    let mut in_comment = false;
    let mut i = 0usize;

    while i < b.len() {
        let c = b[i];

        // ---- 注释 `<!-- ... -->` ----
        if !in_comment && c == b'<' && i + 3 < b.len() && &b[i..i + 4] == b"<!--" {
            in_comment = true;
            st.comments += 1;
            i += 4;
            continue;
        }
        if in_comment {
            if c == b'-' && i + 2 < b.len() && &b[i..i + 3] == b"-->" {
                in_comment = false;
                i += 3;
            } else {
                i += 1;
            }
            continue;
        }

        // ---- 标签 ----
        if c == b'<' {
            let mut j = i + 1;
            let closing = j < b.len() && b[j] == b'/';
            if closing {
                j += 1;
            }
            if j < b.len() && (b[j] == b'!' || b[j] == b'?') {
                // doctype / 处理指令：跳到 '>'
                while j < b.len() && b[j] != b'>' {
                    j += 1;
                }
                i = j.saturating_add(1);
                continue;
            }
            let name = tag_name_of(&String::from_utf8_lossy(&b[j..(j + 32).min(b.len())]));
            if name.is_empty() {
                // 裸 '<'（不是标签）→ 当作文本
                i += 1;
                continue;
            }
            // 脚本/样式块：跳过其内容（不计入文本量，避免把代码当正文）
            if !closing && name == "script" {
                in_script = true;
                st.scripts += 1;
            }
            if !closing && name == "style" {
                in_style = true;
                st.styles += 1;
            }
            if closing && name == "script" {
                in_script = false;
            }
            if closing && name == "style" {
                in_style = false;
            }
            let is_media = MEDIA_TAGS.contains(&name.as_str());
            let is_inter = INTERACTIVE_TAGS.contains(&name.as_str());
            if is_media {
                st.media += 1;
            }
            if is_inter {
                st.interactive += 1;
                if name == "a" {
                    // 只把"带 href 的 a"算链接：向后看一小段
                    let end = (j + 200).min(b.len());
                    let seg = String::from_utf8_lossy(&b[j..end]).to_ascii_lowercase();
                    if seg.contains("href") {
                        st.links += 1;
                    }
                }
            }
            let void = VOID_TAGS.contains(&name.as_str());
            if !closing {
                st.elements += 1;
                if !unique.contains(&name) {
                    unique.push(name.clone());
                }
                if !void {
                    // 给"父层"记一个子元素，然后自己入栈
                    if let Some(top) = child_stack.last_mut() {
                        *top += 1;
                    }
                    child_stack.push(0);
                    tags.push(name.clone());
                    let d = child_stack.len() as u32;
                    if d > max_depth {
                        max_depth = d;
                    }
                } else if let Some(top) = child_stack.last_mut() {
                    *top += 1;
                }
            } else {
                // 闭合：出栈
                if let Some(pos) = tags.iter().rposition(|t| *t == name) {
                    tags.truncate(pos);
                    // 结算被关闭层的子元素数
                    while child_stack.len() > pos + 1 {
                        if let Some(c) = child_stack.pop() {
                            child_counts.push(c);
                        }
                    }
                } else {
                    // 无匹配闭合（畸形 HTML）→ 忽略，不 panic
                }
            }
            // 跳到 '>' （跳过属性）
            while j < b.len() && b[j] != b'>' {
                j += 1;
            }
            i = j.saturating_add(1);
            continue;
        }

        // ---- 文本 ----
        // 按**字符**计数：跳过 UTF-8 续接字节（0b10xxxxxx），否则中文会被按字节放大 3 倍
        if !in_script && !in_style && !is_ws(c) && (c & 0xC0) != 0x80 {
            st.text_chars += 1;
        }
        i += 1;
    }

    // 收尾：把未闭合层也结算（畸形 HTML 不能丢统计）
    while let Some(c) = child_stack.pop() {
        child_counts.push(c);
    }
    st.max_depth = max_depth;
    st.unique_tags = unique.len() as u32;
    st.max_children = child_counts.iter().copied().max().unwrap_or(0);
    let parents = child_counts.len().max(1) as f64;
    let total_children: u32 = child_counts.iter().sum();
    st.avg_children = total_children as f64 / parents;
    st
}

/// **扫描 CSS 源码**（关键字计数；不解析语法树）。
pub fn scan_css(css: &str) -> CssStats {
    let lower = css.to_ascii_lowercase();
    let mut st = CssStats::default();
    st.rules = lower.matches('{').count() as u32;
    st.media_queries = lower.matches("@media").count() as u32;
    st.selectors = lower.matches(',').count() as u32 + st.rules;
    for kw in ["display:flex", "display: flex", "display:grid", "display: grid", "columns", "gap"] {
        st.layout_decls += lower.matches(kw).count() as u32;
    }
    for kw in ["animation", "transition", "transform", "keyframes"] {
        st.motion_decls += lower.matches(kw).count() as u32;
    }
    for kw in ["color", "background", "fill", "stroke", "box-shadow"] {
        st.color_decls += lower.matches(kw).count() as u32;
    }
    st
}

/// 水的半饱和系数（按"每元素平均字符数"计：约 12.5 字/元素到半饱和）。
pub const WATER_K: f64 = 0.08;

/// **源码 → 四场**（本模块主入口）。
///
/// 公式（可解释、可复现；专家给定的方向 + 饱和归一）：
/// ```text
/// 地 = saturate((深度/8) · (最大扇出/4) · 标签多样性, k=2.0)
/// 水 = saturate(文本字符 / 元素数,              k=WATER_K=0.08)
/// 火 = saturate(媒体数 / 元素数  + css动效权重,  k=3.0)
/// 风 = saturate(交互数 / 元素数  + css布局权重,  k=3.0)
/// ```
pub fn parse_source(html: &str, css: &str) -> FieldReading {
    let h = scan_html(html);
    let c = scan_css(css);
    parse_source_stats(&h, &c)
}

/// 由已扫描的统计量算四场（便于测试与复用，避免重复扫描）。
pub fn parse_source_stats(h: &SourceStats, c: &CssStats) -> FieldReading {
    let n = (h.elements.max(1)) as f64;

    // 地：结构度
    let depth_term = (h.max_depth as f64 / 8.0).min(1.0);
    // 扇出用**最大子元素数**（列表/画廊/导航这类"宽度"才体现结构复杂度；
    // 深链页的平均子数恒接近 1，会低估结构度 —— v0.114 由四页面验收测试暴露）
    let sib_term = (h.max_children as f64 / 4.0).min(1.0);
    let diversity = (h.unique_tags as f64 / 24.0).min(1.0);
    let earth = saturate(depth_term * sib_term * diversity, 2.0);

    // 水：正文量
    let water = saturate(h.text_chars as f64 / n, WATER_K);

    // 火：媒体密度（+ CSS 动效作为"动态刺激"的弱加权）
    let media_ratio = h.media as f64 / n;
    let motion = (c.motion_decls as f64 / 20.0).min(0.5);
    let fire = saturate(media_ratio + 0.3 * motion, 3.0);

    // 风：交互密度（+ CSS 布局/媒体查询作为"变化性"的弱加权）
    let inter_ratio = h.interactive as f64 / n;
    let layout = ((c.layout_decls + c.media_queries * 2) as f64 / 30.0).min(0.5);
    let wind = saturate(inter_ratio + 0.3 * layout, 3.0);

    let confidence = confidence_of_source(h);
    FieldReading { earth, water, fire, wind, confidence }
}

/// 源码解析的置信度：**有元素且有内容**才可信。
pub fn confidence_of_source(h: &SourceStats) -> f64 {
    let elems = saturate(h.elements as f64, 0.06); // 约 17 个元素到半饱和
    let text = saturate(h.text_chars as f64, 0.005); // 约 200 字到半饱和
    (0.55 * elems + 0.45 * text).clamp(0.0, 1.0)
}

/// 源码粗分类（**用与渲染版同一套词表** `ContentClass`，不自创分类）。
pub fn classify_source(h: &SourceStats) -> ContentClass {
    if h.elements == 0 && h.text_chars < 50 {
        return ContentClass::Unknown;
    }
    let n = h.elements.max(1) as f64;
    let media_ratio = h.media as f64 / n;
    let inter_ratio = h.interactive as f64 / n;
    let link_ratio = h.links as f64 / n;
    let text_per_elem = h.text_chars as f64 / n;
    if inter_ratio >= 0.25 && text_per_elem < 40.0 {
        return ContentClass::Tool;
    }
    if inter_ratio >= 0.12 && text_per_elem < 150.0 {
        return ContentClass::Commerce;
    }
    if link_ratio >= 0.5 && text_per_elem < 60.0 {
        return ContentClass::Social;
    }
    if media_ratio >= 0.5 {
        return ContentClass::Media;
    }
    if text_per_elem >= 30.0 {
        return ContentClass::Article;
    }
    ContentClass::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== 验收清单第二层：四个测试页面 =====

    /// 纯文本页：大量 `<p>`，无图片、无链接。
    fn text_page() -> String {
        let mut s = String::from("<html><body><article>");
        for i in 0..20 {
            s.push_str(&format!("<p>这是第{i}段正文，用来提供足够的文本量，让水质充分上升。</p>"));
        }
        s.push_str("</article></body></html>");
        s
    }
    /// 纯图片页：大量 `<img>`，几乎无文本。
    fn image_page() -> String {
        let mut s = String::from("<html><body><div class=\"grid\">");
        for i in 0..30 {
            s.push_str(&format!("<img src=\"{i}.jpg\">"));
        }
        s.push_str("</div></body></html>");
        s
    }
    /// 链接密集页：大量 `<a href>`，少量文本。
    fn link_page() -> String {
        let mut s = String::from("<html><body><nav>");
        for i in 0..40 {
            s.push_str(&format!("<a href=\"/p/{i}\">链接</a>"));
        }
        s.push_str("</nav></body></html>");
        s
    }
    /// 结构复杂页：深层嵌套 + 多种标签。
    fn complex_page() -> String {
        "<html><body><header><nav><ul><li><a href=\"#\">A</a></li><li><a href=\"#\">B</a></li></ul></nav></header>\
<section><article><div><table><thead><tr><th>H</th></tr></thead><tbody><tr><td>D</td></tr></tbody></table></div>\
<aside><form><label>L</label><input></form></aside></article></section>\
<footer><p>© 2026</p></footer>\
<div><span><em><strong><i><b>深</b></i></strong></em></span></div></body></html>"
            .to_string()
    }

    #[test]
    fn acceptance_text_page_is_water_dominant() {
        let h = scan_html(&text_page());
        let f = parse_source_stats(&h, &CssStats::default());
        assert!(f.water > 0.7, "水应高: {}", f.water);
        assert!(f.fire < 0.1, "火应低: {}", f.fire);
        assert!(f.wind < 0.1, "风应低: {}", f.wind);
        assert!(f.earth < 0.5, "地不应高: {}", f.earth);
    }

    #[test]
    fn acceptance_image_page_is_fire_dominant() {
        let h = scan_html(&image_page());
        let f = parse_source_stats(&h, &CssStats::default());
        assert!(f.fire > 0.7, "火应高: {}", f.fire);
        assert!(f.water < 0.1, "水应低: {}", f.water);
    }

    #[test]
    fn acceptance_link_page_is_wind_dominant() {
        let h = scan_html(&link_page());
        let f = parse_source_stats(&h, &CssStats::default());
        assert!(f.wind > 0.6, "风应高: {}", f.wind);
    }

    #[test]
    fn acceptance_complex_page_is_earth_high() {
        let h = scan_html(&complex_page());
        let f = parse_source_stats(&h, &CssStats::default());
        assert!(f.earth > 0.5, "地应高: {}", f.earth);
        assert!(h.max_depth >= 5, "嵌套深度识别: {}", h.max_depth);
        assert!(h.unique_tags >= 8, "标签多样性: {}", h.unique_tags);
    }

    #[test]
    fn four_pages_are_mutually_distinguishable() {
        let pages = [text_page(), image_page(), link_page(), complex_page()];
        let mut readings = Vec::new();
        for p in &pages {
            readings.push(parse_source_stats(&scan_html(p), &CssStats::default()));
        }
        // 任意两页的主导场不应完全一致
        let dominants: Vec<usize> = readings.iter().map(|r| r.dominant().0).collect();
        let uniq = {
            let mut v = dominants.clone();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        assert!(uniq >= 3, "四页应至少呈现 3 种主导场: {dominants:?}");
        // 文本页 vs 图片页：水/火互换
        assert!(readings[0].water > readings[1].water);
        assert!(readings[1].fire > readings[0].fire);
    }

    // ===== 饱和曲线与边界 =====

    #[test]
    fn saturate_is_bounded_monotone_and_robust() {
        assert!((saturate(0.0, 5.0) - 0.0).abs() < 1e-12);
        assert!(saturate(1.0, 5.0) > saturate(0.5, 5.0), "单调");
        assert!(saturate(1e9, 5.0) <= 1.0, "有界");
        assert_eq!(saturate(-3.0, 5.0), 0.0, "负输入归零");
        assert_eq!(saturate(f64::NAN, 5.0), 0.0, "NaN 安全");
        assert_eq!(saturate(f64::INFINITY, 5.0), 1.0);
        assert_eq!(saturate(1.0, -1.0), 0.0, "非法 k 安全");
        assert_eq!(saturate(1.0, f64::NAN), 0.0);
    }

    #[test]
    fn scan_handles_malformed_and_hostile_input() {
        let cases = [
            "",
            "<",
            "<<<<>>>>",
            "<div><span>未闭合",
            "</div></div></div>",
            "<!-- 未结束注释",
            "<script>var a = '<div>';</script>",
            "<style>.x{color:red}</style>",
            "\u{0}\u{1}\u{2}乱码\u{fffd}",
            "<img src=x><img><img>",
            "<a href=>x</a>",
            "<DIV CLASS=\"X\">大写标签</DIV>",
        ];
        for c in cases {
            let h = scan_html(c);
            let f = parse_source_stats(&h, &CssStats::default());
            for v in f.to_array() {
                assert!((0.0..=1.0).contains(&v), "越界 {c:?} → {v}");
            }
            assert!((0.0..=1.0).contains(&f.confidence));
        }
    }

    #[test]
    fn script_and_style_content_is_not_counted_as_text() {
        let with_code = "<html><body><script>var s = '很多很多很多很多很多很多很多字';</script></body></html>";
        let h = scan_html(with_code);
        assert!(h.text_chars < 20, "脚本内容不应计入正文: {}", h.text_chars);
        assert_eq!(h.scripts, 1);
        let with_style = "<style>.a{color:red}</style>";
        assert_eq!(scan_html(with_style).styles, 1);
    }

    #[test]
    fn void_tags_do_not_inflate_depth() {
        let h = scan_html("<div><img><img><img><br><hr></div>");
        assert_eq!(h.elements, 6, "元素计数含空元素");
        assert!(h.max_depth <= 1, "空元素不入栈: {}", h.max_depth);
        assert_eq!(h.media, 3);
    }

    #[test]
    fn css_stats_drive_fire_and_wind() {
        let html = "<html><body><div><p>文本</p></div></body></html>";
        let plain = parse_source("html", "");
        assert!(plain.fire < 0.5 && plain.wind < 0.5, "无 CSS 时不应被抬高");
        let dynamic_css = "@media (max-width:600px){.a{display:flex;animation:spin 1s}}\
.x{display:grid;transition:all .2s}.y{transform:scale(2)}@media print{.b{columns:2}}";
        let c = scan_css(dynamic_css);
        assert!(c.media_queries >= 2 && c.layout_decls >= 3 && c.motion_decls >= 3, "{c:?}");
        let f = parse_source(html, dynamic_css);
        assert!(f.wind > 0.0 || f.fire > 0.0, "动效/布局应产生弱加权");
        assert!((0.0..=1.0).contains(&f.fire) && (0.0..=1.0).contains(&f.wind));
    }

    #[test]
    fn confidence_grows_with_content_and_is_zero_for_empty() {
        assert!(confidence_of_source(&SourceStats::default()) < 0.05);
        let big = scan_html(&text_page());
        assert!(confidence_of_source(&big) > 0.8, "{}", confidence_of_source(&big));
    }

    #[test]
    fn classification_uses_shared_vocabulary() {
        assert_eq!(classify_source(&scan_html(&image_page())), ContentClass::Media);
        let _ = classify_source(&scan_html(&text_page()));
        let _ = classify_source(&scan_html(&link_page()));
        let _ = classify_source(&scan_html(&complex_page()));
    }

    #[test]
    fn huge_input_does_not_blow_up() {
        let mut s = String::from("<html><body>");
        for i in 0..20_000 {
            s.push_str(&format!("<div><span>{i}</span></div>"));
        }
        s.push_str("</body></html>");
        let h = scan_html(&s);
        assert_eq!(h.elements, 40_000 + 2);
        let f = parse_source_stats(&h, &CssStats::default());
        for v in f.to_array() {
            assert!((0.0..=1.0).contains(&v));
        }
    }
}
