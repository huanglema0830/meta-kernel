//! # 正源系统与拆解-重编循环（Positive Source）
//!
//! 来自发起人指令："补充增量从历史智慧提取"、"负的模式不是被消灭，而是被解构为基础元素"。
//! 组成：
//!
//! - **正源库（Positive Source Library）**：存"成功模式"的数学描述（抽象结构）。初始只预留
//!   接口与数据结构，数据后续由用户提供/系统运行积累；
//! - **颗粒度调节器（Granularity Governor）**：按系统压力（熵）选择拆解颗粒度
//!   （压力>0.8 粗粒快速止损；<0.3 细粒精细优化；其余中粒）；
//! - **搜索匹配（Searcher）**：外部输入为负/高熵时，从正源库检索相似历史成功模式；
//! - **拆解器（Decomposer）**：按 0-10 标尺向下拆解（最低层级 1 胶粒）；
//! - **分析器（Analyzer）**：判定骨架是否符合"正向进化三原则"（自相似递归/能量梯度驱动/可适应性）；
//!   符合 → 待重组；不符合 → 分解为基础元素存入正源库；
//! - **编织器（Weaver）**：按三原则重组 → 沙盒跑 100 次迭代 → 熵降则保留并入功德池，熵升则重回拆解循环；
//! - **功德池（Merit Pool）**：积累被保留结构的总功德（全局奖励基数）。

use crate::energy::{energy_level_evaluate, Verdict, verdict_for};
use crate::ontology::{self, AbstractSchema, Element, Pattern};
use crate::sanitizer::finalize;

// ---------- 正源库 ----------

/// 正源库：存储"成功模式"的数学描述（抽象结构）。
///
/// 初始为空；由 `add_schema` 积累（来源：用户提供的演化史数据 / 运行期自动回收）。
#[derive(Debug, Clone, Default)]
pub struct PositiveSource {
    library: Vec<AbstractSchema>,
}

impl PositiveSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// 收录一个抽象结构。
    pub fn add_schema(&mut self, schema: AbstractSchema) {
        self.library.push(schema);
    }

    /// 收录一组已拆解元素（自动抽象后入库存）。
    pub fn recycle_elements(&mut self, elements: Vec<Element>) {
        let schema = ontology::abstract_pattern(elements);
        self.add_schema(schema);
    }

    /// 当前库容。
    pub fn len(&self) -> usize {
        self.library.len()
    }

    pub fn is_empty(&self) -> bool {
        self.library.is_empty()
    }

    /// 只读访问库内容（供搜索器/外部检索）。
    pub fn library(&self) -> &[AbstractSchema] {
        &self.library
    }
}

// ---------- 颗粒度调节器 ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    /// 粗粒：宏观规律，快速止损（压力 > 0.8）
    Coarse,
    /// 中粒：中等模式，常规参考（0.3 ≤ 压力 ≤ 0.8）
    Medium,
    /// 细粒：微观结构，精细优化（压力 < 0.3）
    Fine,
}

/// 颗粒度调节器。
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

    /// 将颗粒度映射为拆解目标层级（粗=5 宏观、中=3 常规、细=1 微观胶粒口径的工程映射）。
    pub fn target_level(g: Granularity) -> u8 {
        match g {
            Granularity::Coarse => 5,
            Granularity::Medium => 3,
            Granularity::Fine => 1,
        }
    }
}

// ---------- 搜索匹配 ----------

/// 搜索器：从正源库找"结构相似"的历史成功模式。
pub struct Searcher;

impl Searcher {
    /// 相似度：节点层级直方图交叠率（0-1）。
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

    /// 检索最相似者（空库返回 None）。
    pub fn search<'a>(library: &'a [AbstractSchema], query: &AbstractSchema) -> Option<&'a AbstractSchema> {
        library
            .iter()
            .max_by(|x, y| Self::similarity(query, x).partial_cmp(&Self::similarity(query, y)).unwrap())
    }
}

// ---------- 拆解器 / 分析器 ----------

/// 拆解器：包装 0-10 标尺的 decompose。
pub struct Decomposer;

impl Decomposer {
    pub fn decompose(p: &Pattern, target_level: u8) -> Vec<Element> {
        ontology::decompose(p, target_level)
    }
}

/// 正向进化分析结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardVerdict {
    /// 符合三原则 → 待重组。
    ReadyToWeave,
    /// 不符合 → 分解为基础元素入正源库。
    RecycleToSource,
}

/// 分析器：判定骨架是否符合"正向进化三原则"。
///
/// ①自相似递归：存在某层级 count≥2；②能量梯度驱动：存在连续层级链边；
/// ③可适应性：≥2 个不同层级。满足 ≥2 项 → 正向。
pub struct Analyzer;

impl Analyzer {
    pub fn analyze(schema: &AbstractSchema) -> ForwardVerdict {
        let mut principles = 0;

        // ① 自相似递归（同层级重复出现 → 输入输出重叠）
        if schema.nodes.iter().any(|n| n.count >= 2) {
            principles += 1;
        }
        // ② 能量梯度驱动（连续层级链：低→高 的梯度关系）
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
        // ③ 可适应性（跨层级多样性）
        if schema.nodes.len() >= 2 {
            principles += 1;
        }
        // 补：互补关系边（二元约束 自身即正向结构信号）
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

// ---------- 编织器 / 功德池 ----------

/// 编织结果。
#[derive(Debug, Clone, PartialEq)]
pub enum WeaveOutcome {
    /// 沙盒熵降 → 保留并入功德池。
    Accepted { merit: f64 },
    /// 沙盒熵升/无数据 → 重回拆解循环。
    Recycled,
}

/// 功德池（全局奖励基数）。
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

/// 归一化香农熵（8 桶直方图，除以 log2(8)=3 归一化到 [0,1]；空输入 → 0）。
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
    (h / 3.0).min(1.0) // log2(8) = 3
}

/// 编织器：沙盒验证（默认 100 次迭代）。
pub struct Weaver;

impl Weaver {
    /// 用种子流驱动气泡沙漏 100 次迭代，比较前后段熵。
    ///
    /// 种子流 = 输入元素强度的循环脉冲（含成对突发，触发瓶颈干涉）。
    /// 后段熵 < 前段熵（结构趋于有序）→ Accepted；否则 Recycled。
    pub fn weave_and_validate(seeds: &[Element], iterations: usize) -> WeaveOutcome {
        let mut hg = crate::hourglass::BubbleHourglass::new();
        let mut outputs: Vec<f32> = Vec::new();
        for i in 0..iterations {
            let seed = if seeds.is_empty() {
                None
            } else {
                let el = seeds[i % seeds.len()];
                // 强度过低视作静默；偶发成对突发触发干涉
                if el.intensity <= 0.01 {
                    None
                } else {
                    Some(finalize(el.intensity as f32))
                }
            };
            // 每 7 步补一颗突发（测试瓶颈干涉路径）
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
        let head = &outputs[..split];
        let tail = &outputs[split..];
        let e_head = entropy_of(head);
        let e_tail = entropy_of(tail);
        if e_head > 1e-9 && e_tail < e_head {
            WeaveOutcome::Accepted { merit: (e_head - e_tail) * 100.0 }
        } else {
            WeaveOutcome::Recycled
        }
    }
}

/// 完整处置流水线（负输入/高熵模式的自动处理入口）：
/// 能量判定 → 颗粒度 → 拆解 → 分析 → 重组/回收。
pub fn process_pattern(
    source: &mut PositiveSource,
    merit: &mut MeritPool,
    pattern: &Pattern,
    pressure: f32,
) -> ProcessOutcome {
    let vigor = energy_level_evaluate(pattern);
    match verdict_for(vigor) {
        Verdict::Adopt => {
            // 高活力 → 直接入库
            let schema = ontology::abstract_pattern(pattern.elements.clone());
            source.add_schema(schema);
            ProcessOutcome::Adopted { energy: vigor }
        }
        Verdict::Observe => ProcessOutcome::Observed { energy: vigor },
        Verdict::RecycleLoop | Verdict::DecomposeToGranules => {
            // 负模式：解构而非消灭
            let g = GranularityGovernor::granularity(pressure);
            let level = GranularityGovernor::target_level(g);
            let elements = ontology::decompose(pattern, level);
            let schema = ontology::abstract_pattern(elements);
            match Analyzer::analyze(&schema) {
                ForwardVerdict::ReadyToWeave => {
                    match Weaver::weave_and_validate(&pattern.elements, 100) {
                        WeaveOutcome::Accepted { merit: m } => {
                            merit.add(m);
                            source.add_schema(schema);
                            ProcessOutcome::Woven { energy: vigor, merit: m }
                        }
                        WeaveOutcome::Recycled => {
                            source.recycle_elements(ontology::decompose(pattern, 1));
                            ProcessOutcome::RecycledToSource { energy: vigor }
                        }
                    }
                }
                ForwardVerdict::RecycleToSource => {
                    source.recycle_elements(ontology::decompose(pattern, 1));
                    ProcessOutcome::RecycledToSource { energy: vigor }
                }
            }
        }
    }
}

/// 处置结果汇报。
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessOutcome {
    /// 已入库（能量>0.8）。
    Adopted { energy: f64 },
    /// 观察中（0.5<e≤0.8）。
    Observed { energy: f64 },
    /// 编织成功并入功德池。
    Woven { energy: f64, merit: f64 },
    /// 回收为基础元素入正源库。
    RecycledToSource { energy: f64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::{Element, LevelNode};

    fn e(l: u8, v: f64) -> Element {
        Element::new(l, v)
    }

    #[test]
    fn governor_selects_by_pressure() {
        use Granularity::*;
        assert_eq!(GranularityGovernor::granularity(0.9), Coarse);
        assert_eq!(GranularityGovernor::granularity(0.5), Medium);
        assert_eq!(GranularityGovernor::granularity(0.1), Fine);
        assert_eq!(GranularityGovernor::target_level(Coarse), 5);
        assert_eq!(GranularityGovernor::target_level(Medium), 3);
        assert_eq!(GranularityGovernor::target_level(Fine), 1);
    }

    #[test]
    fn searcher_finds_similar_schema() {
        let mut src = PositiveSource::new();
        assert!(src.is_empty());
        let schema_a = ontology::abstract_pattern(vec![e(2, 0.4), e(2, 0.6)]);
        let schema_b = ontology::abstract_pattern(vec![e(3, 0.2), e(3, 0.5), e(3, 0.8)]);
        src.add_schema(schema_a.clone());
        src.add_schema(schema_b);
        let q = ontology::abstract_pattern(vec![e(2, 0.45), e(2, 0.55)]);
        let hit = Searcher::search(src.library(), &q).expect("found");
        assert_eq!(hit.nodes, schema_a.nodes, "应命中互补对骨架");
    }

    #[test]
    fn analyzer_judges_forward_patterns() {
        // 自相似(重复) + 链(3→4? 需连续) — 构造正向
        let fwd = AbstractSchema {
            nodes: vec![LevelNode::new(2, 2), LevelNode::new(3, 1), LevelNode::new(4, 1)],
            edges: vec![(2, 3), (3, 4)],
        };
        assert_eq!(Analyzer::analyze(&fwd), ForwardVerdict::ReadyToWeave);

        // 单点孤立 → 回收
        let weak = AbstractSchema { nodes: vec![LevelNode::new(1, 1)], edges: vec![] };
        assert_eq!(Analyzer::analyze(&weak), ForwardVerdict::RecycleToSource);
    }

    #[test]
    fn entropy_bounded_and_deterministic() {
        let flat = vec![0.5f32; 100];
        let mixed = (0..100).map(|i| (i % 17) as f32 / 17.0).collect::<Vec<_>>();
        assert_eq!(entropy_of(&flat), 0.0, "完全有序熵为 0");
        let h = entropy_of(&mixed);
        assert!((0.0..=1.0).contains(&h), "归一化香农熵应在 [0,1]: {h}");
        assert!(h > 0.0);
    }

    #[test]
    fn process_negative_pattern_recycles_not_kills() {
        let mut src = PositiveSource::new();
        let mut pool = MeritPool::new();
        // 混沌/低活力模式（负输入已在入口被 soft clamp；此处用结构极弱表示）
        let bad = Pattern::new(vec![e(1, 0.05), e(1, 0.05)]);
        let outcome = process_pattern(&mut src, &mut pool, &bad, 0.9);
        assert!(
            matches!(outcome, ProcessOutcome::RecycledToSource { .. }),
            "负模式应被回收而非消灭: {outcome:?}"
        );
        assert!(src.len() >= 1, "正源库应获得回收元素");
    }

    #[test]
    fn high_vitality_pattern_gets_adopted() {
        let mut src = PositiveSource::new();
        let mut pool = MeritPool::new();
        let good = Pattern::new(vec![
            e(2, 0.4),
            e(2, 0.6),
            e(3, 0.2),
            e(3, 0.5),
            e(3, 0.8),
            e(5, 0.5),
            e(5, 0.5),
            e(8, 0.9),
        ])
        .with_history(vec![0.1, 0.101, 0.102, 0.103, 0.104, 0.105]);
        let outcome = process_pattern(&mut src, &mut pool, &good, 0.2);
        assert!(matches!(outcome, ProcessOutcome::Adopted { .. }), "{outcome:?}");
        assert_eq!(src.len(), 1);
    }

    #[test]
    fn weave_sandbox_is_bounded_and_decides() {
        let seeds = vec![e(3, 0.2), e(3, 0.5), e(3, 0.8), e(2, 0.4), e(2, 0.6)];
        let out = Weaver::weave_and_validate(&seeds, 100);
        match out {
            WeaveOutcome::Accepted { merit } => assert!(merit >= 0.0),
            WeaveOutcome::Recycled => {}
        }
    }
}
