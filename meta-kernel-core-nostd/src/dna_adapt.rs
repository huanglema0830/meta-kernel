//! 基因内核 · 四层自适应入口（dna_adapt）。
//!
//! 统一模式：**场域感知 → 痕迹匹配 → 命中即调用（瞬间学习）｜未命中则生长**：
//! 试探扰动 → 观察响应 → 归纳规律 → 生成适配器 → 验证 → 存入痕迹库。
//!
//! 各层补足（发起人指令）：
//! | 层 | 自适应内容 | 匹配/生长 |
//! |---|---|---|
//! | L1 场域感知 | 新环境七维特征 | 命中调用已有适配器 / 生长新适配器 |
//! | L2 自适应接口 | 宿主环境与可用接口（C ABI/WASM/系统调用/端口，抽象为接口能力签名） | 命中即对接 / 生长新桥接器 |
//! | L3 自适应协议 | 网络环境（TCP/UDP/QUIC/量子通道，抽象为协议能力签名） | 命中即通信 / 生长新协议栈 |
//! | L4 自适应场域 | 设备场域特征 | 命中调用已有本底场 / 持续采集建立专属本底场 |
//! | L5 自适应基线 | 无预设基线的对象 | 主动试探提取正常波动范围 → 基线 |
//!
//! 纯逻辑：真实探测（IO）由宿主/native 探针提供采样，本模块只做学习与决策。

//! 【2.3b 片5 迁移】与 `meta-kernel-core/src/dna_adapt.rs` **同源**；仅作下述适配，其余**逐行逐字未改**：
//! ① 补 `alloc`/`core` 的 `use`（no_std 下 `Vec`/`vec!`/`Box`/`String`/`ToString`/`format!`/集合不在 prelude）
//! ② `std::` 路径 → `core::`/`alloc::`（同一类型或同一常量，**零语义变化**）
//! ③ 引入 `FloatOps` trait ⇒ 浮点方法在 no_std 下解析到 `fmath`（调用点一行未改）
//! ④ 本片清单**由 `coordination/tools/check_migration_closure.py` 的 `plan_core()` 脚本产出**（D40），**不手写**
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;

use crate::dna_generate::{grow, GeneratedAdapter, ProbeResult};
use crate::dna_trace::{bump, match_trace, signature_of, store, AdaptKind, Trace};

/// 自适应层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    L1FieldSense,
    L2Interface,
    L3Protocol,
    L4Baseline,
    L5Baseline,
}

impl Layer {
    pub fn kind(&self) -> AdaptKind {
        match self {
            Layer::L1FieldSense => AdaptKind::FieldSense,
            Layer::L2Interface => AdaptKind::InterfaceAbi,
            Layer::L3Protocol => AdaptKind::ProtocolStack,
            Layer::L4Baseline => AdaptKind::BaselineField,
            Layer::L5Baseline => AdaptKind::BaselineDerive,
        }
    }
}

/// 自适应结果。
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// 命中已有痕迹：立即调用（瞬间完成，无需生长）。
    Reused { trace_id: u32, hits: u32 },
    /// 未命中：生长出新适配器并存库。
    Grown { trace_id: u32, adapter: GeneratedAdapter },
    /// 生长但验证未通过：不入库，需更多试探采样。
    NeedMoreSamples { learned_from: usize },
}

/// 自适应日志（可溯源）。
#[derive(Clone, Debug, PartialEq)]
pub struct AdaptLog {
    pub layer: Layer,
    pub outcome: Outcome,
    pub note: String,
}

/// 四层自适应主入口。
/// - `sig_raw`：七维场域/环境特征原始值；`weights` 学习空间权重；`tol` 匹配容差；
/// - `samples`：试探采样（刺激→响应）；`tests`：验证采样（未参与归纳）；
/// - 命中 → Reused（hits+1，瞬间调用）；未命中 → grow + verify → 通过则入库（Grown）。
pub fn adapt(
    lib: &mut Vec<Trace>,
    layer: Layer,
    sig_raw: &[f64; 7],
    weights: &[f64; 7],
    tol: f64,
    samples: &[ProbeResult],
    tests: &[ProbeResult],
) -> AdaptLog {
    let sig = signature_of(sig_raw, weights);
    if let Some(idx) = match_trace(lib, &sig, tol) {
        bump(lib, idx);
        let t = lib[idx];
        return AdaptLog {
            layer,
            outcome: Outcome::Reused { trace_id: t.id, hits: t.hits },
            note: "痕迹命中——调用既有适配器（学习瞬间完成）".to_string(),
        };
    }
    // 生长路径
    let next_id = lib.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let adapter = grow(samples, tests, sig, next_id, 1e-6);
    if adapter.verified {
        let id = store(lib, sig, layer.kind());
        return AdaptLog {
            layer,
            outcome: Outcome::Grown { trace_id: id, adapter },
            note: "未命中——已生长新适配器并验证通过，存入痕迹库".to_string(),
        };
    }
    AdaptLog {
        layer,
        outcome: Outcome::NeedMoreSamples { learned_from: adapter.learned_from },
        note: "未命中且验证未过——继续试探采样后再生长".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w() -> [f64; 7] {
        [1.0; 7]
    }

    #[test]
    fn first_time_grows_then_reuses_instantly() {
        let mut lib = Vec::new();
        let sig = [1.0; 7];
        let samples = [
            ProbeResult { stimulus: 1.0, response: 2.0 },
            ProbeResult { stimulus: 2.0, response: 4.0 },
            ProbeResult { stimulus: 3.0, response: 6.0 },
        ];
        let tests = [ProbeResult { stimulus: 4.0, response: 8.0 }];
        // 首次：生长（线性 2x）
        let log1 = adapt(&mut lib, Layer::L1FieldSense, &sig, &w(), 0.2, &samples, &tests);
        match log1.outcome {
            Outcome::Grown { adapter, .. } => assert!(adapter.verified),
            other => panic!("首次应生长: {other:?}"),
        }
        assert_eq!(lib.len(), 1);
        // 再次：命中并瞬间复用
        let log2 = adapt(&mut lib, Layer::L1FieldSense, &sig, &w(), 0.2, &samples, &tests);
        match log2.outcome {
            Outcome::Reused { hits, .. } => assert_eq!(hits, 1),
            other => panic!("二次应复用: {other:?}"),
        }
        assert_eq!(lib.len(), 1, "复用不新增痕迹");
    }

    #[test]
    fn unverified_growth_does_not_enter_library() {
        let mut lib = Vec::new();
        let samples = [
            ProbeResult { stimulus: 1.0, response: 2.0 },
            ProbeResult { stimulus: 2.0, response: 4.0 },
            ProbeResult { stimulus: 3.0, response: 6.0 },
        ];
        // 归纳 Linear(2x)：对刺激 3 预测 6，实测 100 → 验证不过
        let bad_tests = [ProbeResult { stimulus: 3.0, response: 100.0 }];
        let log = adapt(&mut lib, Layer::L3Protocol, &[2.5; 7], &w(), 0.2, &samples, &bad_tests);
        assert!(matches!(log.outcome, Outcome::NeedMoreSamples { .. }));
        assert!(lib.is_empty(), "验证未过不得入库（不妄语）");
    }

    #[test]
    fn layers_use_their_kinds() {
        let mut lib = Vec::new();
        let samples = [
            ProbeResult { stimulus: 1.0, response: 1.0 },
            ProbeResult { stimulus: 2.0, response: 2.0 },
            ProbeResult { stimulus: 3.0, response: 3.0 },
        ];
        let tests = [ProbeResult { stimulus: 4.0, response: 4.0 }];
        let _ = adapt(&mut lib, Layer::L4Baseline, &[3.0; 7], &w(), 0.2, &samples, &tests);
        assert_eq!(lib[0].kind, AdaptKind::BaselineField);
        let _ = adapt(&mut lib, Layer::L5Baseline, &[9.0; 7], &w(), 0.2, &samples, &tests);
        assert_eq!(lib[1].kind, AdaptKind::BaselineDerive);
    }

    #[test]
    fn l5_baseline_adaptive_map_via_adapt() {
        // L5 补足演示：无预设基线 → 试探（不同负载下的波动）→ 归纳 → 入库为 BaselineDerive
        let mut lib = Vec::new();
        let samples = [
            ProbeResult { stimulus: 0.2, response: 0.9 }, // 低负载 → 场域低
            ProbeResult { stimulus: 0.5, response: 1.0 }, // 中负载 → 场域正常
            ProbeResult { stimulus: 0.8, response: 1.1 }, // 高负载 → 场域高
        ];
        let tests = [ProbeResult { stimulus: 0.35, response: 0.95 }];
        let log = adapt(&mut lib, Layer::L5Baseline, &[5.0; 7], &w(), 0.2, &samples, &tests);
        assert!(matches!(log.outcome, Outcome::Grown { .. }), "{log:?}");
        assert_eq!(lib.len(), 1);
    }
}
