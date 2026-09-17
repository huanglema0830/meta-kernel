//! 基因内核 · 痕迹库（dna_trace）。
//!
//! 核心原则：**内核不是功能集合，内核是学习能力**；功能从痕迹中生长。
//! 学习是瞬间的：痕迹匹配 = 模式识别，匹配到即立即调用（O(n) 最近邻，库小且热）。
//! 本模块纯逻辑（零依赖、无 IO）：痕迹库存/匹配/命中计数；持久化由宿主负责。

//! 【2.3b 片4 迁移】与 `meta-kernel-core/src/dna_trace.rs` **同源**；仅作下述适配，其余**逐行逐字未改**：
//! ① 补 `alloc`/`core` 的 `use`（no_std 下 `Vec`/`vec!`/`String`/`ToString`/`format!` 不在 prelude）
//! ② `std::cmp::Ordering` → `core::cmp::Ordering`（同一类型）
//! ③ 引入 `FloatOps` trait ⇒ 浮点方法在 no_std 下解析到 `fmath`（调用点一行未改）
//! 说明：本片由「用户指定的 5 模块」**扩为 9 模块** —— 原 5 个**反向依赖** `trace`/`dna_generate`/`dna_trace`/`gene_library`，**不封闭就编不过**（见报告 §三）
use alloc::vec::Vec;
#[allow(unused_imports)] // host(std) 下内在方法优先 ⇒ 本 import 可能"未使用"，这是 FloatOps 机制的必然结果
use crate::fmath::FloatOps;

/// 适配器种类（生长出来的能力形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdaptKind {
    /// 场域感知适配（L1）。
    FieldSense,
    /// 接口/ABI 桥接（L2）。
    InterfaceAbi,
    /// 协议栈（L3）。
    ProtocolStack,
    /// 场域本底（L4）。
    BaselineField,
    /// 基线提取（L5）。
    BaselineDerive,
    /// 通用适配。
    Generic,
}

/// 痕迹条目：场域签名 → 适配器（模式识别即匹配）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    pub id: u32,
    /// 归一化的场域签名（七维；权重由调用方给，见 `signature_of`）。
    pub signature: [f64; 7],
    pub kind: AdaptKind,
    /// 命中次数（使用即生长）。
    pub hits: u32,
}

/// 加权归一签名：s_i × w_i（学习用同一权重的签名空间做匹配）。
pub fn signature_of(s: &[f64; 7], weights: &[f64; 7]) -> [f64; 7] {
    let mut out = [0.0; 7];
    for i in 0..7 {
        out[i] = s[i] * weights[i];
    }
    out
}

/// 欧氏距离（签名空间）。
pub fn distance(a: &[f64; 7], b: &[f64; 7]) -> f64 {
    let mut acc = 0.0;
    for i in 0..7 {
        let d = a[i] - b[i];
        acc += d * d;
    }
    acc.sqrt()
}

/// 痕迹匹配：返回最邻近且距离 ≤ tol 的条目下标（模式识别即匹配）。
/// 匹配成功 → 调用方立即调用该适配器（学习瞬间完成）。
pub fn match_trace(lib: &[Trace], sig: &[f64; 7], tol: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, t) in lib.iter().enumerate() {
        let d = distance(&t.signature, sig);
        if d <= tol && best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// 入库：新增痕迹（返回 id；id 递增，纯函数式——调用方持有 Vec）。
pub fn store(lib: &mut Vec<Trace>, sig: [f64; 7], kind: AdaptKind) -> u32 {
    let id = lib.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    lib.push(Trace { id, signature: sig, kind, hits: 0 });
    id
}

/// 命中计数 +1（使用即生长；供调用方在复用后调用）。
pub fn bump(lib: &mut [Trace], idx: usize) {
    if let Some(t) = lib.get_mut(idx) {
        t.hits = t.hits.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w() -> [f64; 7] {
        [1.0; 7]
    }

    #[test]
    fn store_and_match_hit() {
        let mut lib = Vec::new();
        let sig = signature_of(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], &w());
        let id = store(&mut lib, sig, AdaptKind::FieldSense);
        assert_eq!(id, 1);
        // 相近签名 → 命中
        let probe = signature_of(&[1.02, 0.99, 1.0, 1.0, 1.01, 1.0, 1.0], &w());
        let m = match_trace(&lib, &probe, 0.15);
        assert!(m.is_some(), "近似签名应命中（瞬间匹配）");
        bump(&mut lib, m.unwrap());
        assert_eq!(lib[0].hits, 1);
    }

    #[test]
    fn matcher_misses_far_signature() {
        let mut lib = Vec::new();
        store(&mut lib, signature_of(&[1.0; 7], &w()), AdaptKind::FieldSense);
        let far = signature_of(&[9.0; 7], &w());
        assert!(match_trace(&lib, &far, 0.15).is_none(), "远签名不命中 → 走上生长路径");
    }

    #[test]
    fn nearest_wins() {
        let mut lib = Vec::new();
        store(&mut lib, signature_of(&[1.0; 7], &w()), AdaptKind::FieldSense);
        store(&mut lib, signature_of(&[2.0; 7], &w()), AdaptKind::BaselineField);
        let probe = signature_of(&[1.9; 7], &w());
        let idx = match_trace(&lib, &probe, 0.5).expect("应命中");
        assert_eq!(lib[idx].kind, AdaptKind::BaselineField, "最近邻胜出");
    }

    #[test]
    fn ids_increase_monotonically() {
        let mut lib = Vec::new();
        assert_eq!(store(&mut lib, [0.0; 7], AdaptKind::Generic), 1);
        assert_eq!(store(&mut lib, [0.0; 7], AdaptKind::Generic), 2);
        assert_eq!(store(&mut lib, [0.0; 7], AdaptKind::Generic), 3);
    }
}
