//! 基因库（gene_library）—— 基因内核的核心资产：**存"怎么变化"的公式库**。
//!
//! 设计见 `docs/GENE_LIBRARY_DESIGN.md`（v1.0）。四层结构：
//!
//! ```text
//! 基因库
//! ├─ 基础公式（元素层）      ← 元素周期表关键节点       （同类元素共享）
//! ├─ 场景公式（场景层）      ← 不同场景下的本底场       （场景参数化）
//! ├─ 计算关系（公式层）      ← 公式之间的运算关系       （公式本身）
//! └─ 验证记录（哈希链）      ← 公式的验证历史          （不可篡改）
//! ```
//!
//! - **存储方式 = 公式存储**：记录"怎么变化"（`Formula`），而非"结论是什么"；
//! - **学习机制**：痕迹匹配 → 命中调用（瞬间）｜未命中 → 生长（试探→归纳→验证→入库）；
//! - **哈希链**：每次入库/验证追加一条链节（FNV-1a 前向链），任何历史被改都会失配。
//!
//! 纯逻辑（零依赖、无 IO）：持久化由宿主负责（native 文件系统 / 网关）；内核保持 wasm 可编。
//! 与既有实现的衔接：学习复用 [`crate::dna_generate`]（归纳）与 [`crate::dna_trace`]（签名/匹配）；
//! 本模块把"痕迹"重新组织为"四层公式库"，并补齐计算关系层与哈希链验证层。

//! 【2.3b 片4 迁移】与 `meta-kernel-core/src/gene_library.rs` **同源**；仅作下述适配，其余**逐行逐字未改**：
//! ① 补 `alloc`/`core` 的 `use`（no_std 下 `Vec`/`vec!`/`String`/`ToString`/`format!` 不在 prelude）
//! ② `std::cmp::Ordering` → `core::cmp::Ordering`（同一类型）
//! ③ 引入 `FloatOps` trait ⇒ 浮点方法在 no_std 下解析到 `fmath`（调用点一行未改）
//! 说明：本片由「用户指定的 5 模块」**扩为 9 模块** —— 原 5 个**反向依赖** `trace`/`dna_generate`/`dna_trace`/`gene_library`，**不封闭就编不过**（见报告 §三）
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::format;

use crate::dna_generate::{grow, ProbeResult, Rule};
use crate::dna_trace::{distance, signature_of};
use crate::l5_baseline::BaselineField;

// ===== 四层标识 =====

/// 基因库的四层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneLayer {
    /// 第 1 层 · 基础公式（元素层）。
    Base,
    /// 第 2 层 · 场景公式（场景层）。
    Scene,
    /// 第 3 层 · 计算关系（公式层）。
    Relation,
    /// 第 4 层 · 验证记录（哈希链）。
    Verification,
}

impl GeneLayer {
    fn code(&self) -> u8 {
        match self {
            GeneLayer::Base => 1,
            GeneLayer::Scene => 2,
            GeneLayer::Relation => 3,
            GeneLayer::Verification => 4,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            GeneLayer::Base => "基础公式（元素层）",
            GeneLayer::Scene => "场景公式（场景层）",
            GeneLayer::Relation => "计算关系（公式层）",
            GeneLayer::Verification => "验证记录（哈希链）",
        }
    }
    /// 由编码还原（持久化解码用）。
    pub fn from_code(c: u8) -> Self {
        match c {
            2 => GeneLayer::Scene,
            3 => GeneLayer::Relation,
            4 => GeneLayer::Verification,
            _ => GeneLayer::Base,
        }
    }
}

// ===== 编解码辅助（零依赖）=====

/// 七维签名 → `a,b,c,d,e,f,g`。
fn sig_enc(s: &[f64; 7]) -> String {
    s.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
}

fn sig_dec(s: &str) -> Option<[f64; 7]> {
    let mut out = [0.0f64; 7];
    let mut n = 0;
    for t in s.split(',') {
        if n >= 7 { break; }
        out[n] = t.trim().parse().ok()?;
        n += 1;
    }
    Some(out)
}

fn params_enc(p: &[f64; 4]) -> String {
    p.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
}

fn params_dec(s: &str) -> Option<[f64; 4]> {
    let mut out = [0.0f64; 4];
    let mut n = 0;
    for t in s.split(',') {
        if n >= 4 { break; }
        out[n] = t.trim().parse().ok()?;
        n += 1;
    }
    Some(out)
}

/// 把解码出的字符串转成 `&'static str`。
/// 说明：基因库条目字段为 `&'static str`；**加载一次配置**而故意泄漏，规模可控（条数有限）。
fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

// ===== 公式：记录"怎么变化" =====

/// 公式（变化规律的表示；与结论无关）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Formula {
    /// 线性变化：`y = a·x + b`。
    Linear { a: f64, b: f64 },
    /// 阈值变化：`x ∈ [lo, hi] → y = action`，否则 `0`。
    Threshold { lo: f64, hi: f64, action: f64 },
    /// 直通/恒定：`y = x`。
    PassThrough,
    /// 归一加权合成：`y = Σ wᵢ·xᵢ`（多输入；如 `火 = (a+t)/2`）。
    Weighted { dims: u8, w: [f64; 7] },
    /// **常量公式**：`y = v`（基础公式层承载"判据常量"，如 L4 黄金阈值）。
    Constant { v: f64 },
}

impl Formula {
    /// 单输入求值。
    pub fn eval(&self, x: f64) -> f64 {
        match *self {
            Formula::Linear { a, b } => a * x + b,
            Formula::Threshold { lo, hi, action } => {
                if x >= lo && x <= hi { action } else { 0.0 }
            }
            Formula::PassThrough => x,
            Formula::Weighted { dims, w } => {
                let mut acc = 0.0;
                for i in 0..(dims as usize).min(7) {
                    acc += w[i];
                }
                acc
            }
            Formula::Constant { v } => v,
        }
    }

    /// 多输入求值（`Weighted` 用；其它形态取首元素走 [`Formula::eval`]）。
    pub fn eval_vec(&self, xs: &[f64; 7]) -> f64 {
        match *self {
            Formula::Weighted { dims, w } => {
                let mut acc = 0.0;
                for i in 0..(dims as usize).min(7) {
                    acc += w[i] * xs[i];
                }
                acc
            }
            Formula::Constant { v } => v,
            other => other.eval(xs[0]),
        }
    }

    /// 由 [`Rule`]（dna_generate 归纳产物）转公式。
    pub fn from_rule(r: &Rule) -> Self {
        match *r {
            Rule::Linear { a, b } => Formula::Linear { a, b },
            Rule::Threshold { lo, hi, action } => Formula::Threshold { lo, hi, action },
            Rule::PassThrough => Formula::PassThrough,
        }
    }

    /// 公式名称（可读；用于溯源 note）。
    pub fn name(&self) -> &'static str {
        match self {
            Formula::Linear { .. } => "linear",
            Formula::Threshold { .. } => "threshold",
            Formula::PassThrough => "passthrough",
            Formula::Weighted { .. } => "weighted",
            Formula::Constant { .. } => "constant",
        }
    }

    /// 编码（持久化用；零依赖、单行、冒号分隔）。
    pub fn encode(&self) -> String {
        match *self {
            Formula::Linear { a, b } => format!("lin:{a}:{b}"),
            Formula::Threshold { lo, hi, action } => format!("thr:{lo}:{hi}:{action}"),
            Formula::PassThrough => "pass".to_string(),
            Formula::Constant { v } => format!("const:{v}"),
            Formula::Weighted { dims, w } => {
                let ws = w.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
                format!("w:{}:{}", dims, ws)
            }
        }
    }

    /// 解码（与 [`Formula::encode`] 对偶）。
    pub fn decode(s: &str) -> Option<Self> {
        let p: Vec<&str> = s.split(':').collect();
        match p.first().copied()? {
            "lin" => Some(Formula::Linear { a: p.get(1)?.parse().ok()?, b: p.get(2)?.parse().ok()? }),
            "thr" => Some(Formula::Threshold {
                lo: p.get(1)?.parse().ok()?,
                hi: p.get(2)?.parse().ok()?,
                action: p.get(3)?.parse().ok()?,
            }),
            "pass" => Some(Formula::PassThrough),
            "const" => Some(Formula::Constant { v: p.get(1)?.parse().ok()? }),
            "w" => {
                let dims: u8 = p.get(1)?.parse().ok()?;
                let mut w = [0.0; 7];
                for (i, t) in p.get(2)?.split(',').enumerate() {
                    if i >= 7 { break; }
                    w[i] = t.parse().ok()?;
                }
                Some(Formula::Weighted { dims, w })
            }
            _ => None,
        }
    }
}

/// 公式条目（基础公式层 / 计算关系层共用）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormulaGene {
    pub id: u32,
    pub layer: GeneLayer,
    pub formula: Formula,
    /// 匹配签名（七维；学习空间）。
    pub signature: [f64; 7],
    /// 命中次数（使用即生长）。
    pub hits: u32,
    /// 名称（计算关系层的可读名；基础公式层可留 ""）。
    pub name: &'static str,
}

/// 场景公式条目（场景层：本底场按场景参数化）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneGene {
    pub id: u32,
    /// 场景标识（由 [`crate::l5_context::Context::scene_id`] 派生）。
    pub scene_id: u32,
    pub object: &'static str,
    /// 场景参数（类型/时间/历史/环境 四要素的数值编码）。
    pub params: [f64; 4],
    /// 参数化后的本底场（内部标准地图）。
    pub base: BaselineField,
    pub hits: u32,
}

/// 验证记录链节（哈希链）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainLink {
    /// 序号（从 1 起）。
    pub seq: u32,
    /// 前一条的哈希（首条为 0）。
    pub prev_hash: u64,
    /// 本条哈希 = fnv1a(prev_hash ‖ gene_id ‖ layer ‖ seq ‖ note)。
    pub hash: u64,
    pub gene_id: u32,
    pub layer: GeneLayer,
    pub note: &'static str,
}

// ===== 哈希（零依赖 FNV-1a 64）=====

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64：以 `seed` 为初始状态，吸收 `bytes`。
pub fn fnv1a64(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

fn link_hash(prev: u64, gene_id: u32, layer: GeneLayer, seq: u32, note: &str) -> u64 {
    let mut h = fnv1a64(FNV_OFFSET, &prev.to_le_bytes());
    h = fnv1a64(h, &gene_id.to_le_bytes());
    h = fnv1a64(h, &[layer.code()]);
    h = fnv1a64(h, &seq.to_le_bytes());
    h = fnv1a64(h, note.as_bytes());
    h
}

// ===== 学习结果 =====

/// 基因库学习结果。
#[derive(Clone, Debug, PartialEq)]
pub enum LearnOutcome {
    /// 命中已有公式：立即调用（瞬间完成）。
    Reused { gene_id: u32, hits: u32, layer: GeneLayer },
    /// 未命中：生长出新公式并验证通过，存入对应层（新基因）。
    Grown { gene_id: u32, layer: GeneLayer, formula: Formula, chain_hash: u64 },
    /// 生长但验证未过：不入库，需更多试探采样。
    NeedMoreSamples { learned_from: usize },
}

// ===== 基因库 =====

/// 基因库（四层结构 + 哈希链）。
#[derive(Clone, Debug, Default)]
pub struct GeneLibrary {
    /// 第 1 层 · 基础公式（元素层）。
    pub base: Vec<FormulaGene>,
    /// 第 2 层 · 场景公式（场景层）。
    pub scene: Vec<SceneGene>,
    /// 第 3 层 · 计算关系（公式层）。
    pub relation: Vec<FormulaGene>,
    /// 第 4 层 · 验证记录（哈希链）。
    pub chain: Vec<ChainLink>,
}

impl GeneLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// 各层条目数 `[基础, 场景, 计算关系, 验证]`。
    pub fn sizes(&self) -> [usize; 4] {
        [self.base.len(), self.scene.len(), self.relation.len(), self.chain.len()]
    }

    fn next_id(v: &[FormulaGene]) -> u32 {
        v.iter().map(|g| g.id).max().unwrap_or(0) + 1
    }

    /// 在某层内做最近邻匹配（返回下标）。
    fn match_in(v: &[FormulaGene], sig: &[f64; 7], tol: f64) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, g) in v.iter().enumerate() {
            let d = distance(&g.signature, sig);
            if d <= tol && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }

    /// 学习（学习机制主入口）：
    /// 痕迹匹配 → 命中则 `Reused`（公式命中即调用）；未命中则生长（归纳→验证→入库 `Grown`）。
    /// 入库后自动向**验证记录（哈希链）**追加一条链节。
    pub fn learn(
        &mut self,
        sig_raw: &[f64; 7],
        weights: &[f64; 7],
        tol: f64,
        samples: &[ProbeResult],
        tests: &[ProbeResult],
        layer: GeneLayer,
    ) -> LearnOutcome {
        let sig = signature_of(sig_raw, weights);
        let target = match layer {
            GeneLayer::Base => &mut self.base,
            GeneLayer::Relation => &mut self.relation,
            // 场景层由 learn_scene 负责；误调则落到基础公式层语义
            GeneLayer::Scene => &mut self.base,
            GeneLayer::Verification => &mut self.base,
        };
        if let Some(idx) = Self::match_in(target, &sig, tol) {
            target[idx].hits = target[idx].hits.saturating_add(1);
            let g = target[idx];
            return LearnOutcome::Reused { gene_id: g.id, hits: g.hits, layer };
        }
        // 未命中 → 生长
        let next_id = Self::next_id(target).max(1);
        let adapter = grow(samples, tests, sig, next_id, 1e-6);
        if !adapter.verified {
            return LearnOutcome::NeedMoreSamples { learned_from: adapter.learned_from };
        }
        let formula = Formula::from_rule(&adapter.rule);
        let id = Self::next_id(target);
        target.push(FormulaGene { id, layer, formula, signature: sig, hits: 0, name: "" });
        let hash = self.append_verification(id, layer, formula.name());
        LearnOutcome::Grown { gene_id: id, layer, formula, chain_hash: hash }
    }

    /// 场景公式入库（场景层）：同场景标识 → 复用（hits+1）；否则新增参数化本底场。
    pub fn learn_scene(
        &mut self,
        scene_id: u32,
        object: &'static str,
        base: BaselineField,
        params: [f64; 4],
    ) -> (u32, bool) {
        if let Some(s) = self.scene.iter_mut().find(|s| s.scene_id == scene_id) {
            s.hits = s.hits.saturating_add(1);
            return (s.id, true);
        }
        let id = self.scene.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        self.scene.push(SceneGene { id, scene_id, object, params, base, hits: 0 });
        (id, false)
    }

    /// 取场景公式（按场景标识）。
    pub fn scene_of(&self, scene_id: u32) -> Option<&SceneGene> {
        self.scene.iter().find(|s| s.scene_id == scene_id)
    }

    /// 新增计算关系（公式层）：公式之间的运算关系。
    pub fn add_relation(&mut self, name: &'static str, expr: Formula, signature: [f64; 7]) -> u32 {
        let id = self.relation.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        self.relation.push(FormulaGene {
            id,
            layer: GeneLayer::Relation,
            formula: expr,
            signature,
            hits: 0,
            name,
        });
        id
    }

    /// **基础公式层 · upsert 命名公式**（通用版：常量/加权/线性…皆可）。
    /// 这是"公式存储"的通用入口——L1 场域映射库等经此登记映射公式。
    pub fn set_base_formula(&mut self, name: &'static str, f: Formula, sig: [f64; 7]) -> u32 {
        if let Some(g) = self.base.iter_mut().find(|g| g.name == name && g.layer == GeneLayer::Base) {
            g.formula = f;
            return g.id;
        }
        let id = self.base.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        self.base.push(FormulaGene {
            id,
            layer: GeneLayer::Base,
            formula: f,
            signature: sig,
            hits: 0,
            name,
        });
        id
    }

    /// **基础公式层 · 读取命名公式**（未命中返回 `None` → 调用方回退内置缺省）。
    pub fn base_formula(&self, name: &str) -> Option<Formula> {
        self.base
            .iter()
            .find(|g| g.name == name && g.layer == GeneLayer::Base)
            .map(|g| g.formula)
    }

    /// **关系层（公式层）· upsert 命名公式**（v0.112：四元组 `P→ΔQ` 等"公式间关系"存此层）。
    pub fn set_relation_formula(&mut self, name: &'static str, f: Formula, sig: [f64; 7]) -> u32 {
        if let Some(g) = self.relation.iter_mut().find(|g| g.name == name) {
            g.formula = f;
            return g.id;
        }
        let id = self.relation.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        self.relation.push(FormulaGene {
            id,
            layer: GeneLayer::Relation,
            formula: f,
            signature: sig,
            hits: 0,
            name,
        });
        id
    }

    /// **关系层 · 读取命名公式**（未命中 → `None`）。
    pub fn relation_formula(&self, name: &str) -> Option<Formula> {
        self.relation.iter().find(|g| g.name == name).map(|g| g.formula)
    }

    /// **基础公式层 · upsert 常量公式**（供 L4 等读取"判据常量"）。
    /// 同名已存在则更新其值（**改基因库即改判据**）；否则新增。
    pub fn set_base_constant(&mut self, name: &'static str, v: f64, sig: [f64; 7]) -> u32 {
        self.set_base_formula(name, Formula::Constant { v }, sig)
    }

    /// **基础公式层 · 读取常量公式的值**（未命中返回 `None` → 调用方回退内置缺省）。
    pub fn base_constant(&self, name: &str) -> Option<f64> {
        self.base
            .iter()
            .find(|g| g.name == name && g.layer == GeneLayer::Base)
            .and_then(|g| match g.formula {
                Formula::Constant { v } => Some(v),
                other => Some(other.eval(1.0)),
            })
    }

    // ===== 持久化（纯文本编解码；零依赖；宿主负责落盘/读回）=====

    fn field(s: &str) -> String {
        s.replace(['\t', '\n', '\r'], " ")
    }

    /// 编码为**单行制文本**（4 层全含；供宿主写文件）。
    pub fn to_text(&self) -> String {
        let mut o = String::new();
        o.push_str("# genelib v1\n");
        for g in &self.base {
            o.push_str(&format!(
                "base\t{}\t{}\t{}\t{}\t{}\n",
                g.id,
                Self::field(g.name),
                g.formula.encode(),
                sig_enc(&g.signature),
                g.hits
            ));
        }
        for s in &self.scene {
            o.push_str(&format!(
                "scene\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                s.id,
                s.scene_id,
                Self::field(s.object),
                params_enc(&s.params),
                s.base.earth,
                s.base.water,
                s.base.fire,
                s.base.wind,
                Self::field(s.base.established),
                s.hits
            ));
        }
        for g in &self.relation {
            o.push_str(&format!(
                "relation\t{}\t{}\t{}\t{}\t{}\n",
                g.id,
                Self::field(g.name),
                g.formula.encode(),
                sig_enc(&g.signature),
                g.hits
            ));
        }
        for l in &self.chain {
            o.push_str(&format!(
                "chain\t{}\t{}\t{}\t{}\t{}\t{}\n",
                l.seq,
                l.prev_hash,
                l.hash,
                l.gene_id,
                l.layer.code(),
                Self::field(l.note)
            ));
        }
        o
    }

    /// 解码（宽松：忽略空行/未知行；头部不匹配返回 `None`）。
    pub fn from_text(t: &str) -> Option<Self> {
        let mut lib = GeneLibrary::new();
        let mut lines = t.lines();
        let head = lines.next()?.trim_end_matches(['\r', '\n']);
        if !head.starts_with("# genelib v1") {
            return None;
        }
        for raw in lines {
            let line = raw.trim_end_matches(['\r', '\n']);
            if line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            match f.first().copied() {
                Some("base") if f.len() >= 6 => {
                    lib.base.push(FormulaGene {
                        id: f[1].parse().ok()?,
                        layer: GeneLayer::Base,
                        formula: Formula::decode(f[3])?,
                        signature: sig_dec(f[4])?,
                        hits: f[5].parse().ok()?,
                        name: leak(f[2]),
                    });
                }
                Some("relation") if f.len() >= 6 => {
                    lib.relation.push(FormulaGene {
                        id: f[1].parse().ok()?,
                        layer: GeneLayer::Relation,
                        formula: Formula::decode(f[3])?,
                        signature: sig_dec(f[4])?,
                        hits: f[5].parse().ok()?,
                        name: leak(f[2]),
                    });
                }
                Some("scene") if f.len() >= 11 => {
                    lib.scene.push(SceneGene {
                        id: f[1].parse().ok()?,
                        scene_id: f[2].parse().ok()?,
                        object: leak(f[3]),
                        params: params_dec(f[4])?,
                        base: BaselineField {
                            earth: f[5].parse().ok()?,
                            water: f[6].parse().ok()?,
                            fire: f[7].parse().ok()?,
                            wind: f[8].parse().ok()?,
                            object: leak(f[3]),
                            established: leak(f[9]),
                        },
                        hits: f[10].parse().ok()?,
                    });
                }
                Some("chain") if f.len() >= 7 => {
                    lib.chain.push(ChainLink {
                        seq: f[1].parse().ok()?,
                        prev_hash: f[2].parse().ok()?,
                        hash: f[3].parse().ok()?,
                        gene_id: f[4].parse().ok()?,
                        layer: GeneLayer::from_code(f[5].parse().ok()?),
                        note: leak(f[6]),
                    });
                }
                _ => {}
            }
        }
        Some(lib)
    }

    /// 追加一条验证记录（哈希链）。
    pub fn append_verification(&mut self, gene_id: u32, layer: GeneLayer, note: &'static str) -> u64 {
        let seq = self.chain.len() as u32 + 1;
        let prev = self.chain.last().map(|l| l.hash).unwrap_or(0);
        let hash = link_hash(prev, gene_id, layer, seq, note);
        self.chain.push(ChainLink { seq, prev_hash: prev, hash, gene_id, layer, note });
        hash
    }

    /// 校验哈希链完整性（任何历史被改都会失配）。
    pub fn verify_chain(&self) -> bool {
        let mut prev = 0u64;
        for (i, l) in self.chain.iter().enumerate() {
            if l.seq as usize != i + 1 || l.prev_hash != prev {
                return false;
            }
            let expect = link_hash(prev, l.gene_id, l.layer, l.seq, l.note);
            if expect != l.hash {
                return false;
            }
            prev = l.hash;
        }
        true
    }

    /// 链上最新哈希（供宿主持久化做锚点）。
    pub fn chain_head(&self) -> u64 {
        self.chain.last().map(|l| l.hash).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w() -> [f64; 7] {
        [1.0; 7]
    }

    fn lin_samples() -> [ProbeResult; 3] {
        [
            ProbeResult { stimulus: 1.0, response: 3.0 },
            ProbeResult { stimulus: 2.0, response: 5.0 },
            ProbeResult { stimulus: 3.0, response: 7.0 },
        ]
    }

    #[test]
    fn four_layers_start_empty() {
        let lib = GeneLibrary::new();
        assert_eq!(lib.sizes(), [0, 0, 0, 0]);
        assert!(lib.verify_chain(), "空链视为完整");
    }

    #[test]
    fn learn_grows_then_reuses() {
        let mut lib = GeneLibrary::new();
        let tests = [ProbeResult { stimulus: 4.0, response: 9.0 }];
        let o1 = lib.learn(&[1.0; 7], &w(), 0.2, &lin_samples(), &tests, GeneLayer::Base);
        match o1 {
            LearnOutcome::Grown { formula, layer, .. } => {
                assert_eq!(layer, GeneLayer::Base);
                match formula {
                    Formula::Linear { a, b } => {
                        assert!((a - 2.0).abs() < 1e-9 && (b - 1.0).abs() < 1e-9, "火=2x+1: {formula:?}");
                    }
                    other => panic!("应为线性: {other:?}"),
                }
            }
            other => panic!("首次应生长: {other:?}"),
        }
        assert_eq!(lib.base.len(), 1);
        assert_eq!(lib.chain.len(), 1, "入库即入链");
        // 再次 → 命中复用
        let o2 = lib.learn(&[1.0; 7], &w(), 0.2, &lin_samples(), &tests, GeneLayer::Base);
        match o2 {
            LearnOutcome::Reused { hits, .. } => assert_eq!(hits, 1),
            other => panic!("二次应复用: {other:?}"),
        }
        assert_eq!(lib.base.len(), 1, "复用不新增基因");
        assert_eq!(lib.chain.len(), 1, "复用不写链");
    }

    #[test]
    fn unverified_growth_not_entering_library() {
        let mut lib = GeneLibrary::new();
        // 归纳 2x+1，但验证集给 4→100（不符）
        let bad = [ProbeResult { stimulus: 4.0, response: 100.0 }];
        let o = lib.learn(&[3.0; 7], &w(), 0.2, &lin_samples(), &bad, GeneLayer::Relation);
        assert!(matches!(o, LearnOutcome::NeedMoreSamples { .. }), "{o:?}");
        assert!(lib.relation.is_empty(), "验证未过不入库（不妄语）");
        assert_eq!(lib.chain.len(), 0, "不入库则不入链");
    }

    #[test]
    fn relation_formula_weighted_evals() {
        // 火 = (a + t)/2 → 用 Weighted 表述（维度 7，权重 0.5 于 a/t）
        let mut w7 = [0.0; 7];
        w7[0] = 0.5; // t
        w7[2] = 0.5; // a
        let f = Formula::Weighted { dims: 7, w: w7 };
        let s = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        // 火 = 0.5*t + 0.5*a = 0.5*1 + 0.5*3 = 2.0
        assert!((f.eval_vec(&s) - 2.0).abs() < 1e-9);
        let mut lib = GeneLibrary::new();
        let id = lib.add_relation("火=(a+t)/2", f, s);
        assert_eq!(id, 1);
        assert_eq!(lib.relation.len(), 1);
        assert_eq!(lib.relation[0].name, "火=(a+t)/2");
    }

    #[test]
    fn scene_formula_reuses_by_scene_id() {
        let mut lib = GeneLibrary::new();
        let base = BaselineField { earth: 1.0, water: 1.0, fire: 0.9, wind: 0.7, object: "nb", established: "learned" };
        let (id1, reused1) = lib.learn_scene(42, "nb", base, [1.0, 0.5, 0.0, 0.5]);
        assert_eq!(id1, 1);
        assert!(!reused1, "首次入库");
        let (id2, reused2) = lib.learn_scene(42, "nb", base, [1.0, 0.5, 0.0, 0.5]);
        assert_eq!(id2, 1);
        assert!(reused2, "同场景复用（只切参数不新增条目）");
        assert_eq!(lib.scene.len(), 1);
        assert_eq!(lib.scene_of(42).unwrap().hits, 1);
        assert!(lib.scene_of(99).is_none());
    }

    #[test]
    fn hash_chain_detects_tampering() {
        let mut lib = GeneLibrary::new();
        lib.append_verification(1, GeneLayer::Base, "linear");
        lib.append_verification(2, GeneLayer::Scene, "scene-formula");
        lib.append_verification(3, GeneLayer::Relation, "three-quantities");
        assert_eq!(lib.chain.len(), 3);
        assert!(lib.verify_chain());
        // 篡改中间链节 → 失配
        let mut tampered = lib.clone();
        tampered.chain[1].note = "forged";
        assert!(!tampered.verify_chain(), "历史被改必须可检出");
        // 篡改哈希本身
        let mut tampered2 = lib.clone();
        tampered2.chain[0].hash = 12345;
        assert!(!tampered2.verify_chain());
        // chain_head 稳定
        assert_eq!(lib.chain_head(), lib.chain.last().unwrap().hash);
        assert_ne!(lib.chain_head(), 0);
    }

    #[test]
    fn fnv1a_is_deterministic() {
        assert_eq!(fnv1a64(FNV_OFFSET, b"abc"), fnv1a64(FNV_OFFSET, b"abc"));
        assert_ne!(fnv1a64(FNV_OFFSET, b"abc"), fnv1a64(FNV_OFFSET, b"abd"));
    }

    #[test]
    fn layers_keep_separate_gene_pools() {
        let mut lib = GeneLibrary::new();
        let tests = [ProbeResult { stimulus: 4.0, response: 9.0 }];
        let _ = lib.learn(&[1.0; 7], &w(), 0.1, &lin_samples(), &tests, GeneLayer::Base);
        let _ = lib.learn(&[5.0; 7], &w(), 0.1, &lin_samples(), &tests, GeneLayer::Relation);
        assert_eq!(lib.base.len(), 1);
        assert_eq!(lib.relation.len(), 1, "不同层各自成池，不互相匹配");
        assert_eq!(lib.chain.len(), 2);
    }

    // ===== v0.107：基础公式层常量（供 L4 读判据）=====

    #[test]
    fn base_constant_upsert_and_read() {
        let mut lib = GeneLibrary::new();
        assert_eq!(lib.base_constant("l4.threshold.high"), None, "未登记 → None（调用方回退）");
        let id1 = lib.set_base_constant("l4.threshold.high", 1.618, [1.0; 7]);
        assert_eq!(lib.base_constant("l4.threshold.high"), Some(1.618));
        let id2 = lib.set_base_constant("l4.threshold.high", 2.5, [1.0; 7]);
        assert_eq!(id1, id2, "同名 upsert（不新增）");
        assert_eq!(lib.base.len(), 1);
        assert_eq!(lib.base_constant("l4.threshold.high"), Some(2.5), "改值生效");
    }

    #[test]
    fn constant_formula_encodes_and_evals() {
        let f = Formula::Constant { v: 0.618 };
        assert!((f.eval(999.0) - 0.618).abs() < 1e-12, "常量与输入无关");
        assert!((f.eval_vec(&[1.0; 7]) - 0.618).abs() < 1e-12);
        assert_eq!(f.name(), "constant");
        assert_eq!(Formula::decode(&f.encode()), Some(f), "编解码对偶");
    }

    // ===== v0.107：持久化（编解码 + 重启恢复）=====

    fn rich_library() -> GeneLibrary {
        let mut lib = GeneLibrary::new();
        let tests = [ProbeResult { stimulus: 4.0, response: 9.0 }];
        let _ = lib.learn(&[1.0; 7], &w(), 0.2, &lin_samples(), &tests, GeneLayer::Base);
        let _ = lib.learn(&[5.0; 7], &w(), 0.2, &lin_samples(), &tests, GeneLayer::Relation);
        lib.set_base_constant("l4.threshold.high", 1.618, [1.0; 7]);
        lib.add_relation("火=(a+t)/2", Formula::Weighted { dims: 7, w: [0.5, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0] }, [2.0; 7]);
        let base = BaselineField {
            earth: 1.1, water: 0.9, fire: 2.0, wind: 1.0, object: "nb", established: "learned",
        };
        lib.learn_scene(42, "nb", base, [1.0, 2.0, 1.0, 2.0]);
        lib
    }

    #[test]
    fn persist_roundtrip_restores_all_four_layers() {
        let lib = rich_library();
        let before = lib.sizes();
        let text = lib.to_text();
        assert!(text.starts_with("# genelib v1"), "头部标记");
        let back = GeneLibrary::from_text(&text).expect("可解码");
        assert_eq!(back.sizes(), before, "四层条目数一致");
        assert_eq!(back.chain_head(), lib.chain_head(), "哈希链锚点一致");
        assert!(back.verify_chain(), "重启后哈希链仍可校验");
        // 关键字段还原
        assert_eq!(back.base_constant("l4.threshold.high"), Some(1.618));
        assert_eq!(back.scene_of(42).map(|s| s.object), Some("nb"));
        assert!((back.scene_of(42).unwrap().base.fire - 2.0).abs() < 1e-9);
        assert_eq!(back.relation.iter().filter(|g| g.name == "火=(a+t)/2").count(), 1);
    }

    /// **验收项：基因库重启后能恢复**（含哈希链完整性与新写入能力）。
    #[test]
    fn restored_library_is_usable_and_chain_continues() {
        let lib = rich_library();
        let text = lib.to_text();
        let mut back = GeneLibrary::from_text(&text).unwrap();
        let head = back.chain_head();
        // 恢复后继续写入 → 链在既有锚点上延续
        let h2 = back.append_verification(9, GeneLayer::Relation, "after-restore");
        assert_ne!(h2, head);
        assert_eq!(back.chain.last().unwrap().prev_hash, head, "链节接续（不重置）");
        assert!(back.verify_chain(), "接续后仍完整");
        // 恢复后仍可学习（命中既有条目）
        let tests = [ProbeResult { stimulus: 4.0, response: 9.0 }];
        let o = back.learn(&[1.0; 7], &w(), 0.2, &lin_samples(), &tests, GeneLayer::Base);
        assert!(matches!(o, LearnOutcome::Reused { .. }), "恢复后命中既有公式：{o:?}");
    }

    #[test]
    fn decode_rejects_foreign_or_empty_text() {
        assert!(GeneLibrary::from_text("").is_none(), "空文本不是基因库");
        assert!(GeneLibrary::from_text("hello\nworld").is_none(), "无头部");
        // 头部正确但含未知行 → 宽松忽略，不崩
        let ok = GeneLibrary::from_text("# genelib v1\nunknown\tx\ty\n");
        assert!(ok.is_some());
        assert_eq!(ok.unwrap().sizes(), [0, 0, 0, 0]);
    }

    #[test]
    fn persist_escapes_tabs_and_newlines_in_fields() {
        let mut lib = GeneLibrary::new();
        lib.set_base_constant("name\twith\ttab", 1.0, [1.0; 7]);
        let t = lib.to_text();
        let back = GeneLibrary::from_text(&t).unwrap();
        assert_eq!(back.base.len(), 1, "含制表符的名字不会撑破字段数");
        assert_eq!(back.base[0].name, "name with tab", "制表符被替换为空格");
    }
}
