//! # 正源系统（Positive Source）— Phase 4 场域模型
//!
//! **概念变更（容器 → 搜索器）**：正源库不再是"有边界的容器"，而是系统的
//! **感知与搜索能力**：没有内部/外部边界，只有"可触达/不可触达"；触达范围
//! 随系统运行动态扩展。
//!
//! - **自动搜索**：`search_and_deconstruct(pattern)` —— 搜索可触达范围 →
//!   自动解构 → 自动吸收（无显式 add/recycle 方法）；
//! - **本地缓存**：已解构模式缓存（去重，避免重复拆解）；
//! - **催化剂（1.3）**：`Adopted` 采纳的模式自动标记为催化剂；搜索时
//!   催化剂模式匹配权重 +20%（`CATALYST_BOOST = 1.2`）；
//! - **触达范围分层（2.3）**：L0 系统自身状态（已实现）、L1 本地源
//!   （经 `index_local_source` 接入，模拟文件/DB 扫描）、L2 网络、L3 抽象
//!   知识、L4 宇宙演化史 —— L2-L4 预留位（tier_mask 位图随实现扩展）。
//!
//! 解构引擎见 [`crate::evo_deconstructor`]。

use crate::energy::{energy_level_evaluate, Verdict, verdict_for};
use crate::evo_deconstructor;
use crate::ontology::{self, AbstractSchema, Element, Pattern};
use crate::sanitizer::finalize;

/// 催化剂搜索加权（+20%）。
pub const CATALYST_BOOST: f32 = 1.2;
/// 已知模式识别阈值。
pub const KNOWN_SIMILARITY: f64 = 0.7;
/// 缓存上限。
pub const CACHE_CAP: usize = 64;

// ---------- 颗粒度调节器 / 拆解器 / 分析器（沿用 v1） ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Coarse,
    Medium,
    Fine,
}

pub struct GranularityGovernor;

impl GranularityGovernor {
    /// 按系统压力（熵值 0-1）返回拆解颗粒度。
    pub fn granularity(pressure: f32) -> Granularity {
        let p = finalize(pressure);
        if p > 0.8 {
            Granularity::Coarse
        } else if p < 0.3 {
            Granularity::Fine
        } else {
            Granularity::Medium
        }
    }

    pub fn target_level(g: Granularity) -> u8 {
        match g {
            Granularity::Coarse => 5,
            Granularity::Medium => 3,
            Granularity::Fine => 1,
        }
    }
}

/// 拆解器：包装 0-10 标尺 decompose。
pub struct Decomposer;

impl Decomposer {
    pub fn decompose(p: &Pattern, target_level: u8) -> Vec<Element> {
        ontology::decompose(p, target_level)
    }
}

/// 正向进化分析结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardVerdict {
    ReadyToWeave,
    RecycleToSource,
}

pub struct Analyzer;

impl Analyzer {
    pub fn analyze(schema: &AbstractSchema) -> ForwardVerdict {
        let mut principles = 0;
        if schema.nodes.iter().any(|n| n.count >= 2) {
            principles += 1;
        }
        let mut has_chain = false;
        for w in schema.nodes.windows(2) {
            if w[1].level == w[0].level + 1 {
                has_chain = true;
                break;
            }
        }
        if has_chain {
            principles += 1;
        }
        if schema.nodes.len() >= 2 {
            principles += 1;
        }
        if schema.edges.iter().any(|(a, b)| a != b) {
            principles += 1;
        }
        if principles >= 3 {
            ForwardVerdict::ReadyToWeave
        } else {
            ForwardVerdict::RecycleToSource
        }
    }
}

// ---------- 编织器 / 功德池 / 熵（沿用 v1） ----------

#[derive(Debug, Clone, PartialEq)]
pub enum WeaveOutcome {
    Accepted { merit: f64 },
    Recycled,
}

#[derive(Debug, Clone, Default)]
pub struct MeritPool {
    merit: f64,
}

impl MeritPool {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, points: f64) {
        self.merit += points.max(0.0);
    }
    pub fn total(&self) -> f64 {
        self.merit
    }
}

/// 归一化香农熵（8 桶，除以 log2(8)=3；空 → 0）。
pub fn entropy_of(values: &[f32]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut bins = [0u64; 8];
    for v in values {
        let i = ((finalize(*v) * 8.0) as usize).min(7);
        bins[i] += 1;
    }
    let n = values.len() as f64;
    let h: f64 = bins
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / n;
            -p * p.log2()
        })
        .sum();
    (h / 3.0).min(1.0)
}

pub struct Weaver;

impl Weaver {
    /// 沙盒验证（默认 100 次迭代）：输出熵降则 Accepted。
    pub fn weave_and_validate(seeds: &[Element], iterations: usize) -> WeaveOutcome {
        let mut hg = crate::hourglass::BubbleHourglass::new();
        let mut outputs: Vec<f32> = Vec::new();
        for i in 0..iterations {
            let seed = if seeds.is_empty() {
                None
            } else {
                let el = seeds[i % seeds.len()];
                if el.intensity <= 0.01 {
                    None
                } else {
                    Some(finalize(el.intensity as f32))
                }
            };
            if i % 7 == 0 {
                hg.push(0.7);
                hg.push(0.8);
            }
            let outs = hg.tick(seed);
            outputs.extend(outs);
        }
        let n = outputs.len();
        let split = n / 2;
        if split < 8 {
            return WeaveOutcome::Recycled;
        }
        let e_head = entropy_of(&outputs[..split]);
        let e_tail = entropy_of(&outputs[split..]);
        if e_head > 1e-9 && e_tail < e_head {
            WeaveOutcome::Accepted { merit: (e_head - e_tail) * 100.0 }
        } else {
            WeaveOutcome::Recycled
        }
    }
}

// ---------- 搜索器（支持催化剂加权） ----------

pub struct Searcher;

impl Searcher {
    /// 直方图相似度（0-1）。
    pub fn similarity(query: &AbstractSchema, cand: &AbstractSchema) -> f64 {
        let hist = |s: &AbstractSchema| -> [u64; 11] {
            let mut h = [0u64; 11];
            for n in &s.nodes {
                h[(n.level.min(10)) as usize] += n.count as u64;
            }
            h
        };
        let a = hist(query);
        let b = hist(cand);
        let inter: u64 = a.iter().zip(b.iter()).map(|(x, y)| (*x).min(*y)).sum();
        let denom = a.iter().sum::<u64>().max(b.iter().sum::<u64>()).max(1);
        inter as f64 / denom as f64
    }

    fn is_catalyst(catalysts: &[AbstractSchema], cand: &AbstractSchema) -> bool {
        catalysts.iter().any(|c| *c == *cand)
    }

    /// 普通检索：库内最相似。
    pub fn search<'a>(schemas: &'a [AbstractSchema], query: &AbstractSchema) -> Option<&'a AbstractSchema> {
        schemas
            .iter()
            .max_by(|x, y| Self::similarity(query, x).partial_cmp(&Self::similarity(query, y)).unwrap())
    }

    /// 催化剂加权检索：催化剂模式相似度 ×1.2（权重可超 1，体现"优先催化"）。
    pub fn search_catalyzed<'a>(
        schemas: &'a [AbstractSchema],
        catalysts: &[AbstractSchema],
        query: &AbstractSchema,
    ) -> Option<&'a AbstractSchema> {
        let score = |s: &AbstractSchema| {
            let base = Self::similarity(query, s);
            if Self::is_catalyst(catalysts, s) {
                base * CATALYST_BOOST as f64
            } else {
                base
            }
        };
        schemas.iter().max_by(|x, y| score(x).partial_cmp(&score(y)).unwrap())
    }
}

// ---------- 正源场域 ----------

/// 模式签名（去重缓存键）。
pub fn signature(p: &Pattern) -> u64 {
    let n = p.elements.len() as u64;
    let mean = if p.elements.is_empty() {
        0.0
    } else {
        p.elements.iter().map(|e| e.intensity).sum::<f64>() / p.elements.len() as f64
    };
    let hist_tail = p
        .history
        .iter()
        .rev()
        .take(4)
        .map(|x| (*x * 1000.0) as u64)
        .fold(0u64, |acc, x| acc.wrapping_mul(31).wrapping_add(x));
    n.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ ((mean * 1000.0) as u64).wrapping_mul(0x1000_0000_01B3)
        ^ hist_tail
}

/// 触达层级（2.3）：L0 自状态、L1 本地源已实现；L2-L4 预留。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachMask(u32);

impl Default for ReachMask {
    fn default() -> Self {
        Self::base()
    }
}

impl ReachMask {
    pub const L0_SELF: u32 = 1 << 0;
    pub const L1_LOCAL: u32 = 1 << 1;
    pub const L2_NETWORK: u32 = 1 << 2;
    pub const L3_ABSTRACT: u32 = 1 << 3;
    pub const L4_COSMOS: u32 = 1 << 4;

    pub const fn base() -> Self {
        Self(Self::L0_SELF)
    }
    pub fn enable(&mut self, bit: u32) {
        self.0 |= bit;
    }
    pub const fn mask(&self) -> u32 {
        self.0
    }
    pub const fn has(&self, bit: u32) -> bool {
        self.0 & bit != 0
    }
}

/// 正源场域：感知/搜索能力（非容器）。
#[derive(Debug, Clone, Default)]
pub struct PositiveSource {
    /// 已吸收骨架（本地知识库，去重）。
    absorbed: Vec<AbstractSchema>,
    /// 催化剂子集（被采纳模式的骨架）。
    catalysts: Vec<AbstractSchema>,
    /// 已解构缓存（签名 → 胶粒），避免重复拆解。
    cache: Vec<(u64, Vec<Element>)>,
    /// 层级1 本地数据源（文件/DB 扫描结果模拟）。
    local_source: Vec<Pattern>,
    /// 触达范围位图。
    reach: ReachMask,
}

impl PositiveSource {
    pub fn new() -> Self {
        Self { reach: ReachMask::base(), ..Default::default() }
    }

    /// 接入层级1 本地数据源（模拟扫描文件/数据库得到的模式集）。
    pub fn index_local_source(&mut self, sources: Vec<Pattern>) {
        if sources.is_empty() {
            return;
        }
        self.local_source = sources;
        self.reach.enable(ReachMask::L1_LOCAL);
    }

    /// 当前可触达层级位图（bit0..4 对应 L0..L4）。
    pub fn reachable_levels(&self) -> u32 {
        self.reach.mask()
    }

    /// 已发现路径数（解构缓存条目 + 催化剂数）。
    pub fn path_count(&self) -> u32 {
        (self.cache.len() + self.catalysts.len()) as u32
    }

    /// 已吸收骨架（可触达知识，供检索）。
    pub fn reachable_schemas(&self) -> &[AbstractSchema] {
        &self.absorbed
    }

    /// 催化剂列表（只读）。
    pub fn catalysts(&self) -> &[AbstractSchema] {
        &self.catalysts
    }

    pub fn absorbed_len(&self) -> usize {
        self.absorbed.len()
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.absorbed.is_empty() && self.catalysts.is_empty() && self.cache.is_empty()
    }

    fn absorb_schema(&mut self, schema: AbstractSchema) {
        if !self.absorbed.contains(&schema) {
            self.absorbed.push(schema);
        }
    }

    /// 采纳模式 → 吸收并标记为催化剂（1.3）。
    fn absorb_as_catalyst(&mut self, schema: AbstractSchema) {
        self.absorb_schema(schema.clone());
        if !self.catalysts.contains(&schema) {
            self.catalysts.push(schema);
        }
    }

    fn remember_granules(&mut self, sig: u64, granules: Vec<Element>) -> Vec<Element> {
        if self.cache.len() >= CACHE_CAP {
            self.cache.remove(0);
        }
        self.cache.push((sig, granules.clone()));
        granules
    }

    /// 自动搜索可触达范围并解构模式（层级0 自状态 + 层级1 本地源）。
    ///
    /// 已解构过（签名命中缓存）→ 直接返回缓存胶粒；
    /// 与已知知识高度相似 → 识别并吸收；
    /// 新模式 → 解构 → 吸收 → 缓存。输出均为层级 1 胶粒，可化合。
    pub fn search_and_deconstruct(&mut self, p: &Pattern) -> Vec<Element> {
        let sig = signature(p);

        // ① 缓存命中：避免重复拆解
        if let Some((_, g)) = self.cache.iter().find(|(s, _)| *s == sig) {
            return g.clone();
        }

        let abstract_self = ontology::abstract_pattern(p.elements.clone());

        // ② 层级1：本地源扫描（可触达范围动态扩展）
        if self.reach.has(ReachMask::L1_LOCAL) {
            let mut best: Option<(&Pattern, f64)> = None;
            for lp in &self.local_source {
                let la = ontology::abstract_pattern(lp.elements.clone());
                let sim = Searcher::similarity(&abstract_self, &la);
                if best.map(|(_, s)| sim > s).unwrap_or(true) {
                    best = Some((lp, sim));
                }
            }
            if let Some((lp, sim)) = best {
                if sim >= KNOWN_SIMILARITY {
                    self.absorb_schema(ontology::abstract_pattern(lp.elements.clone()));
                }
            }
        }

        // ③ 已知知识高相似 → 识别（吸收原骨架），仍按自身解构
        if let Some(known) = Searcher::search(&self.absorbed, &abstract_self) {
            if Searcher::similarity(&abstract_self, known) >= KNOWN_SIMILARITY {
                self.absorb_schema(known.clone());
            }
        }

        // ④ 解构（2.4 evo_deconstructor）→ 吸收 → 缓存
        let granules = evo_deconstructor::deconstruct_to_granules(p);
        let low = ontology::decompose(p, 3);
        self.absorb_schema(ontology::abstract_pattern(low));
        self.remember_granules(sig, granules)
    }
}

// ---------- 处置流水线 ----------

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessOutcome {
    Adopted { energy: f64 },
    Observed { energy: f64 },
    Woven { energy: f64, merit: f64 },
    RecycledToSource { energy: f64 },
}

/// 完整处置流水线：能量判定 → 颗粒度 → 拆解 → 分析 → 编织/吸收。
pub fn process_pattern(
    source: &mut PositiveSource,
    merit: &mut MeritPool,
    pattern: &Pattern,
    pressure: f32,
) -> ProcessOutcome {
    let vigor = energy_level_evaluate(pattern);
    match verdict_for(vigor) {
        Verdict::Adopt => {
            // 高活力 → 采纳：自动成为催化剂（1.3）
            let schema = ontology::abstract_pattern(pattern.elements.clone());
            source.absorb_as_catalyst(schema);
            ProcessOutcome::Adopted { energy: vigor }
        }
        Verdict::Observe => ProcessOutcome::Observed { energy: vigor },
        Verdict::RecycleLoop | Verdict::DecomposeToGranules => {
            let g = GranularityGovernor::granularity(pressure);
            let level = GranularityGovernor::target_level(g);
            let elements = ontology::decompose(pattern, level);
            let schema = ontology::abstract_pattern(elements);
            match Analyzer::analyze(&schema) {
                ForwardVerdict::ReadyToWeave => {
                    match Weaver::weave_and_validate(&pattern.elements, 100) {
                        WeaveOutcome::Accepted { merit: m } => {
                            merit.add(m);
                            source.absorb_schema(schema);
                            ProcessOutcome::Woven { energy: vigor, merit: m }
                        }
                        WeaveOutcome::Recycled => {
                            source.search_and_deconstruct(pattern);
                            ProcessOutcome::RecycledToSource { energy: vigor }
                        }
                    }
                }
                ForwardVerdict::RecycleToSource => {
                    source.search_and_deconstruct(pattern);
                    ProcessOutcome::RecycledToSource { energy: vigor }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::Element;

    fn e(l: u8, v: f64) -> Element {
        Element::new(l, v)
    }

    #[test]
    fn governor_selects_by_pressure() {
        use Granularity::*;
        assert_eq!(GranularityGovernor::granularity(0.9), Coarse);
        assert_eq!(GranularityGovernor::granularity(0.5), Medium);
        assert_eq!(GranularityGovernor::granularity(0.1), Fine);
    }

    #[test]
    fn searcher_plain_finds_best() {
        let a = ontology::abstract_pattern(vec![e(2, 0.4), e(2, 0.6)]);
        let b = ontology::abstract_pattern(vec![e(3, 0.2), e(3, 0.5), e(3, 0.8)]);
        let q = ontology::abstract_pattern(vec![e(3, 0.45), e(3, 0.7)]);
        let schemas = [a.clone(), b.clone()];
        let hit = Searcher::search(&schemas, &q).expect("found");
        assert_eq!(hit.nodes, b.nodes);
    }

    #[test]
    fn catalyst_boost_outweighs_plain_match() {
        use crate::ontology::LevelNode;
        // q={2:10}；b 完全命中（sim 1.0）；a 为催化剂、略差（sim 0.9）
        // 普通检索 → b 胜出；催化剂加权 a = 0.9×1.2 = 1.08 > 1.0 → a 胜出
        let mk = |nodes: Vec<LevelNode>| AbstractSchema { nodes, edges: vec![] };
        let a = mk(vec![LevelNode::new(2, 9), LevelNode::new(3, 1)]);
        let b = mk(vec![LevelNode::new(2, 10)]);
        let q = mk(vec![LevelNode::new(2, 10)]);
        let schemas = [a.clone(), b.clone()];
        let catalysts = [a.clone()];
        let plain = Searcher::search(&schemas, &q).unwrap();
        assert_eq!(plain.nodes, b.nodes, "无催化剂时 b 胜出");
        let catalyzed = Searcher::search_catalyzed(&schemas, &catalysts, &q).unwrap();
        assert_eq!(catalyzed.nodes, a.nodes, "催化剂 a 加权 1.2 后胜出");
    }

    #[test]
    fn analyzer_judges_forward_patterns() {
        let fwd = AbstractSchema {
            nodes: vec![ontology::LevelNode::new(2, 2), ontology::LevelNode::new(3, 1), ontology::LevelNode::new(4, 1)],
            edges: vec![(2, 3), (3, 4)],
        };
        assert_eq!(Analyzer::analyze(&fwd), ForwardVerdict::ReadyToWeave);
        let weak = AbstractSchema { nodes: vec![ontology::LevelNode::new(1, 1)], edges: vec![] };
        assert_eq!(Analyzer::analyze(&weak), ForwardVerdict::RecycleToSource);
    }

    #[test]
    fn entropy_bounded_and_deterministic() {
        let flat = vec![0.5f32; 100];
        let mixed = (0..100).map(|i| (i % 17) as f32 / 17.0).collect::<Vec<_>>();
        assert_eq!(entropy_of(&flat), 0.0);
        let h = entropy_of(&mixed);
        assert!((0.0..=1.0).contains(&h));
        assert!(h > 0.0);
    }

    #[test]
    fn search_and_deconstruct_auto_absorbs_and_caches() {
        let mut src = PositiveSource::new();
        let p = Pattern::new(vec![e(6, 0.8), e(2, 0.4), e(2, 0.6)]).with_history(vec![0.1, 0.2, 0.3, 0.4]);
        let g1 = src.search_and_deconstruct(&p);
        assert!(!g1.is_empty());
        assert!(g1.iter().all(|x| x.level <= 1), "胶粒必须 ≤ 层级1");
        let absorbed = src.absorbed_len();
        assert!(absorbed >= 1, "自动吸收");
        // 同模式再次 → 缓存命中（不新增吸收/缓存）
        let g2 = src.search_and_deconstruct(&p);
        assert_eq!(g1, g2);
        assert_eq!(src.absorbed_len(), absorbed);
        assert_eq!(src.cache_len(), 1);
        assert!(src.path_count() >= 1);
    }

    #[test]
    fn local_source_expands_reach_and_absorbs() {
        let mut src = PositiveSource::new();
        assert_eq!(src.reachable_levels(), ReachMask::L0_SELF);
        src.index_local_source(vec![Pattern::new(vec![e(1, 0.05), e(1, 0.05)])]);
        assert_eq!(src.reachable_levels(), ReachMask::L0_SELF | ReachMask::L1_LOCAL);
        // 相似模式 → 层级1 识别并吸收
        let p = Pattern::new(vec![e(1, 0.05), e(1, 0.06)]);
        src.search_and_deconstruct(&p);
        assert!(src.absorbed_len() >= 1);
    }

    #[test]
    fn process_negative_pattern_recycles_not_kills() {
        let mut src = PositiveSource::new();
        let mut pool = MeritPool::new();
        let bad = Pattern::new(vec![e(1, 0.05), e(1, 0.05)]);
        let outcome = process_pattern(&mut src, &mut pool, &bad, 0.9);
        assert!(matches!(outcome, ProcessOutcome::RecycledToSource { .. }), "{outcome:?}");
        assert!(src.absorbed_len() >= 1);
        assert!(src.catalysts().is_empty(), "回收不产生催化剂");
    }

    #[test]
    fn adopted_pattern_becomes_catalyst() {
        let mut src = PositiveSource::new();
        let mut pool = MeritPool::new();
        let good = Pattern::new(vec![
            e(2, 0.4), e(2, 0.6), e(3, 0.2), e(3, 0.5), e(3, 0.8),
            e(5, 0.5), e(5, 0.5), e(8, 0.9),
        ])
        .with_history(vec![0.1, 0.101, 0.102, 0.103, 0.104, 0.105]);
        let outcome = process_pattern(&mut src, &mut pool, &good, 0.2);
        assert!(matches!(outcome, ProcessOutcome::Adopted { .. }), "{outcome:?}");
        assert_eq!(src.catalysts().len(), 1, "采纳 → 自动催化剂");
        assert_eq!(src.absorbed_len(), 1);
    }

    #[test]
    fn weave_sandbox_is_bounded_and_decides() {
        let seeds = vec![e(3, 0.2), e(3, 0.5), e(3, 0.8), e(2, 0.4), e(2, 0.6)];
        match Weaver::weave_and_validate(&seeds, 100) {
            WeaveOutcome::Accepted { merit } => assert!(merit >= 0.0),
            WeaveOutcome::Recycled => {}
        }
    }
}
