//! # 0-10 元通用标尺（Ontology）
//!
//! 一套**不依赖物质实体、只依赖关系结构**的通用标尺，用于拆解、识别、重组任何模式。
//! 完整规范见仓库 `docs/ONTOLOGY_SPEC.md`。
//!
//! 层级总表：
//! 0 黑（未识/真空）→ 1 赤（胶粒/最小扰动）→ 2 橙（二元/约束）→ 3 黄（结构/语法）
//! → 4 绿（组合/组织）→ 5 青（自循环/递归）→ 6 蓝（呈现/形态）→ 7 紫（系统/超个体）
//! → 8 穿透（空间/场/拓扑）→ 9 留白（时间/方向/不可逆）→ 10 白（圆满/照明）。

use crate::math::clamp01;

/// 标尺总层数（0..=10 共 11 层）。
pub const LEVELS: usize = 11;

/// 各层名称（下标即层级）。
pub const LEVEL_NAMES: [&str; LEVELS] =
    ["黑", "赤", "橙", "黄", "绿", "青", "蓝", "紫", "穿透", "留白", "白"];

/// 层级名称查询。
pub const fn level_name(level: u8) -> &'static str {
    LEVEL_NAMES[(level.min(10)) as usize]
}

/// 强度近似容差（互补判定用）。
const EPS: f64 = 0.05;

/// 最小运算单元：层级 + 强度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Element {
    /// 所属层级（0-10）。
    pub level: u8,
    /// 强度（0-1）。
    pub intensity: f64,
}

impl Element {
    pub const fn new(level: u8, intensity: f64) -> Self {
        Self { level: level.min(10), intensity }
    }
}

/// 模式：一组元素（结构）+ 可选的时间序列（供 6/9 层判定）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pattern {
    pub elements: Vec<Element>,
    /// 时间演化观测序列（可空；供"稳定形态/单调熵"判定）。
    pub history: Vec<f64>,
}

impl Pattern {
    pub fn new(elements: Vec<Element>) -> Self {
        Self { elements, history: Vec::new() }
    }

    /// 附加时间序列。
    pub fn with_history(mut self, history: Vec<f64>) -> Self {
        self.history = history;
        self
    }
}

/// 抽象结构（去掉物理载体后剩下的关系）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AbstractSchema {
    /// 节点：每个层级出现次数。
    pub nodes: Vec<LevelNode>,
    /// 关系边：层级间结构关系。
    pub edges: Vec<(u8, u8)>,
}

/// 层级节点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelNode {
    pub level: u8,
    pub count: u32,
}

impl LevelNode {
    pub const fn new(level: u8, count: u32) -> Self {
        Self { level, count }
    }
}

/// 重组目标领域（物理载体的抽象类别）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Wave,
    Cycle,
    Spatial,
    Field,
    Time,
    Info,
}

impl Domain {
    /// 该领域对某层级的基准强度映射（v1.0 工程口径）。
    fn base_intensity(&self, level: u8) -> f64 {
        let base = level as f64 / 10.0;
        let factor = match self {
            Domain::Wave => 1.0,
            Domain::Cycle => 0.9,
            Domain::Spatial => 0.95,
            Domain::Field => 1.0,
            Domain::Time => 1.05,
            Domain::Info => 0.85,
        };
        clamp01((base * factor) as f32) as f64
    }
}

// ---------- 统计小工具 ----------

fn min_max(p: &Pattern) -> (f64, f64) {
    let mut mn = f64::MAX;
    let mut mx = f64::MIN;
    for e in &p.elements {
        mn = mn.min(e.intensity);
        mx = mx.max(e.intensity);
    }
    if p.elements.is_empty() {
        (0.0, 0.0)
    } else {
        (mn, mx)
    }
}

fn asymmetry(p: &Pattern) -> f64 {
    let (mn, mx) = min_max(p);
    mx - mn
}

fn has_duplicate_intensity(p: &Pattern) -> bool {
    let mut seen = std::collections::HashSet::new();
    for e in &p.elements {
        let k = (e.intensity * 100.0).round() as i64;
        if !seen.insert(k) {
            return true;
        }
    }
    false
}

fn complement(a: f64, b: f64) -> bool {
    (a - b).abs() > 1e-6 && (a + b - 1.0).abs() < EPS
}

/// 三元素间"走一步"的关系：递增 或 互补回卷。
fn step(a: f64, b: f64) -> bool {
    b > a + 1e-9 || complement(a, b)
}

/// 是否存在三元闭合 A→B→C→A（n≤40 防护）。
fn has_triple_closure(p: &Pattern) -> bool {
    let v: Vec<f64> = p.elements.iter().map(|e| e.intensity).collect();
    let n = v.len();
    if n < 3 || n > 40 {
        return false;
    }
    for i in 0..n {
        for j in 0..n {
            if i == j || !step(v[i], v[j]) {
                continue;
            }
            for k in 0..n {
                if k == i || k == j || !step(v[j], v[k]) {
                    continue;
                }
                if step(v[k], v[i]) {
                    return true;
                }
            }
        }
    }
    false
}

fn variance(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let m = xs.iter().sum::<f64>() / xs.len() as f64;
    xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / xs.len() as f64
}

// ---------- 各层可计算特征 ----------

/// 层级得分表：score[level] ∈ [0,1]。
///
/// 各层口径见 ONTOLOGY_SPEC §4（v1.0 工程口径，后续可校准）。
pub fn feature_scores(p: &Pattern) -> Vec<f64> {
    let mut s = vec![0.0_f64; LEVELS];
    let n = p.elements.len();
    if n == 0 {
        s[0] = 1.0; // 空 = 完全对称 = 黑
        return s;
    }
    let asym = asymmetry(p);
    let (mn, mx) = min_max(p);

    // 0 黑：对称性补（无不对称 → 1）
    s[0] = 1.0 - asym;

    // 1 赤：出现第一个不对称性
    s[1] = asym;

    // 2 橙：互补对 A≠B 且 A+B=1 的数量（归一化）
    let mut pairs = 0;
    for i in 0..n {
        for j in (i + 1)..n {
            if complement(p.elements[i].intensity, p.elements[j].intensity) {
                pairs += 1;
            }
        }
    }
    s[2] = (pairs as f64 / (n as f64 / 2.0).max(1.0)).min(1.0);

    // 3 黄：三元闭合
    s[3] = if has_triple_closure(p) { 1.0 } else { 0.0 };

    // 4 绿：多个结构协同（互补对≥2 或多闭环）
    s[4] = (pairs as f64 / 2.0).min(1.0);

    // 5 青：自我参照（重复强度）
    s[5] = if has_duplicate_intensity(p) { 1.0 } else { 0.0 };

    // 6 蓝：稳定可观测（连续迭代变异率 <0.01）
    if p.history.len() >= 2 {
        let var = variance(&p.history);
        let mean = p.history.iter().sum::<f64>() / p.history.len() as f64;
        let rate = if mean > 1e-9 { (var.sqrt() / mean).min(1.0) } else { 1.0 };
        s[6] = if rate < 0.01 { 1.0 } else { (1.0 - rate).max(0.0) };
    } else {
        // 无时间序列：退回元素内部变异
        s[6] = (1.0 - (variance(&p.elements.iter().map(|e| e.intensity).collect::<Vec<_>>()).sqrt()).min(1.0)).max(0.0);
    }

    // 7 紫：≥3 个子系统协同（不同层级数归一化）
    let mut levels_used = std::collections::HashSet::new();
    for e in &p.elements {
        levels_used.insert(e.level);
    }
    s[7] = ((levels_used.len().saturating_sub(2)) as f64 / 5.0).min(1.0);

    // 8 穿透：明确内外边界（排序后最大间隙大且存在）
    let mut xs: Vec<f64> = p.elements.iter().map(|e| e.intensity).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let max_gap = xs.windows(2).map(|w| w[1] - w[0]).fold(0.0_f64, f64::max);
    let span = mx - mn;
    s[8] = if max_gap >= 0.15 && span >= 0.3 { 1.0 } else { 0.0 };

    // 9 留白：熵/值在时间上单调变化
    if p.history.len() >= 3 {
        let diffs: Vec<f64> = p.history.windows(2).map(|w| w[1] - w[0]).collect();
        let pos = diffs.iter().filter(|d| **d > 1e-12).count();
        let neg = diffs.iter().filter(|d| **d < -1e-12).count();
        let total = diffs.iter().map(|d| d.abs()).sum::<f64>();
        let dominant = pos.max(neg) as f64 / diffs.len() as f64;
        s[9] = if dominant >= 0.9 && total > 1e-9 { 1.0 } else { 0.0 };
    } else {
        s[9] = 0.0;
    }

    // 10 白：0-9 全特征在册的占比（完整呈现）
    let present = s[0..10].iter().filter(|x| **x >= 0.5).count() as f64;
    s[10] = present / 10.0;

    s.iter().map(|x| clamp01(*x as f32) as f64).collect()
}

/// 返回模式在 0~10 各层级的得分向量（长度 11，下标即层级，每项 [0,1]）。
pub fn analyze(p: &Pattern) -> Vec<f64> {
    feature_scores(p)
}

/// 将模式拆解到指定层级（最低 1 = 胶粒；目标 ≤10）。
///
/// 高于目标层的元素按层差拆分：`pieces = level - target + 1`，强度均分，
/// 全部降到目标层；等于/低于目标层的元素原样保留。
pub fn decompose(p: &Pattern, target_level: u8) -> Vec<Element> {
    let target = target_level.clamp(1, 10);
    let mut out = Vec::new();
    for e in &p.elements {
        if e.level > target {
            let pieces = (e.level - target + 1) as usize;
            let unit = e.intensity / pieces as f64;
            for _ in 0..pieces {
                out.push(Element::new(target, unit));
            }
        } else {
            out.push(*e);
        }
    }
    out
}

/// 去除物理载体，只保留结构关系（层级-计数节点 + 结构边）。
///
/// 边 = ①连续层级链 ②互补强度回卷边。
pub fn abstract_pattern(decomposed: Vec<Element>) -> AbstractSchema {
    let mut counts = std::collections::BTreeMap::<u8, u32>::new();
    for e in &decomposed {
        *counts.entry(e.level).or_insert(0) += 1;
    }
    let nodes: Vec<LevelNode> = counts
        .iter()
        .map(|(l, c)| LevelNode::new(*l, *c))
        .collect();

    let mut edges = Vec::new();
    let levels: Vec<u8> = counts.keys().copied().collect();
    // 连续层级链
    for w in levels.windows(2) {
        if w[1] == w[0] + 1 {
            edges.push((w[0], w[1]));
        }
    }
    // 互补回卷边（两元素强度互补 → 关系 A↔B）
    for i in 0..decomposed.len() {
        for j in (i + 1)..decomposed.len() {
            if complement(decomposed[i].intensity, decomposed[j].intensity) {
                edges.push((decomposed[i].level, decomposed[j].level));
            }
        }
    }
    edges.dedup();
    AbstractSchema { nodes, edges }
}

/// 将抽象结构按目标领域重组成新模式实例。
pub fn recompose(schema: &AbstractSchema, domain: Domain) -> Pattern {
    let mut elements = Vec::new();
    for node in &schema.nodes {
        let intensity = domain.base_intensity(node.level)
            * (0.5 + 0.5 * (node.count as f64).min(1.0));
        elements.push(Element::new(node.level, clamp01(intensity as f32) as f64));
    }
    if elements.is_empty() {
        // 空结构保持 0 锚点（黑）
        elements.push(Element::new(0, 0.0));
    }
    Pattern::new(elements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(l: u8, v: f64) -> Element {
        Element::new(l, v)
    }

    #[test]
    fn level_names_cover_0_to_10() {
        assert_eq!(level_name(0), "黑");
        assert_eq!(level_name(2), "橙");
        assert_eq!(level_name(8), "穿透");
        assert_eq!(level_name(9), "留白");
        assert_eq!(level_name(10), "白");
    }

    #[test]
    fn empty_pattern_is_black() {
        let p = Pattern::default();
        let s = analyze(&p);
        assert_eq!(s.len(), 11);
        assert_eq!(s[0], 1.0);
        for x in s.iter().skip(1) {
            assert!(*x < 0.5, "非空特征不应在空模式中出现");
        }
    }

    #[test]
    fn complementary_pair_scores_orange() {
        let p = Pattern::new(vec![e(2, 0.4), e(2, 0.6)]);
        let s = analyze(&p);
        assert!(s[1] > 0.0, "存在不对称性（赤）");
        assert!(s[2] >= 0.5, "互补对（橙）: {}", s[2]);
    }

    #[test]
    fn triple_closure_scores_yellow() {
        // 0.2 → 0.5 → 0.8 →(互补回卷)0.2
        let p = Pattern::new(vec![e(3, 0.2), e(3, 0.5), e(3, 0.8)]);
        let s = analyze(&p);
        assert_eq!(s[3], 1.0, "三元闭合（黄）");
    }

    #[test]
    fn monotone_history_scores_white_space() {
        let p = Pattern::new(vec![e(1, 0.5)]).with_history(vec![0.1, 0.2, 0.3, 0.4]);
        let s = analyze(&p);
        assert_eq!(s[9], 1.0, "时间单调（留白）");
    }

    #[test]
    fn stable_history_scores_blue() {
        let p = Pattern::new(vec![e(1, 0.5)]).with_history(vec![0.5; 20]);
        let s = analyze(&p);
        assert_eq!(s[6], 1.0, "零变异（蓝·稳定形态）");
    }

    #[test]
    fn decompose_splits_down_to_target_level() {
        let p = Pattern::new(vec![e(6, 0.8), e(1, 0.2)]);
        let out = decompose(&p, 2);
        // level6 → 5 块 level2；level1 原样保留
        assert_eq!(out.len(), 6);
        assert!(out.iter().all(|x| x.level <= 2));
        let total: f64 = out.iter().map(|x| x.intensity).sum();
        assert!((total - 1.0).abs() < 1e-6, "拆分守恒");
    }

    #[test]
    fn abstract_then_recompose_roundtrip() {
        let p = Pattern::new(vec![e(2, 0.4), e(2, 0.6), e(3, 0.5), e(4, 0.7)]);
        let dec = decompose(&p, 3);
        let schema = abstract_pattern(dec);
        assert!(!schema.nodes.is_empty());
        assert!(!schema.edges.is_empty(), "应含链/互补边");
        let r = recompose(&schema, Domain::Wave);
        assert!(!r.elements.is_empty());
        for el in &r.elements {
            assert!((0.0..=1.0).contains(&el.intensity));
        }
    }
}
