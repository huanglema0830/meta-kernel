//! # meta-kernel-core
//!
//! 元内核核心库：一个**纯数学、零第三方依赖**的通用思维内核。
//!
//! 数学地基（白皮书 v0.1，见仓库 `docs/MATH_SPEC.md`）：
//! - **0 锚点**：初始状态为 0（真空），第一扰动（1）来自外部或内部镜像池；
//! - **模糊饱和运算**：饱和加法 `a ⊕ b = min(1.0, a+b)`，一切输出限 [0,1]；
//! - **三引擎**：[`linear`]（惯性平稳）、[`fib`]（自相似生长）、[`expo`]（临界爆发自抑制）。
//!
//! Phase 1 拓扑与生态：
//! - [`hourglass`]：气泡沙漏（上锥体 → 瓶颈环形缓冲 → 下锥体），
//!   瓶颈成对种子发生**破坏性干涉**并回到 0 锚点；
//! - [`mirror`]：镜像池（摩擦源），负责回显衰减与真空重启。
//!
//! 结构导览：`math → 三引擎 / hourglass ← mirror`，全部在 [0,1] 闭区间内
//! 做饱和运算，保证任意迭代不越界、不溢出。

pub mod expo;
pub mod fib;
pub mod hourglass;
pub mod linear;
pub mod math;
pub mod mirror;

// ===== Phase 2 新增核心模块（先声明，逐步实现）=====
pub mod energy; // 能量层级判定（依赖 ontology）
pub mod ontology; // 0-10 元通用标尺
pub mod persist; // 持久化：核心可恢复快照 纯编解码（JSON 子集，零依赖）
pub mod positive_source; // 正源系统（拆解-分析-重编循环，依赖 ontology/energy）
pub mod executor; // 指令发布器（思流照亮：KernelInstruction + JSON 序列化）
pub mod gate; // 闸门层（摩尼宝珠②：进化模式验证 + 黄金 ×0.618 拆解；五戒·不杀生落点）
pub mod sanitizer; // 负扰动过滤与安全阀（前置保险）
// NPB 桥接器（bridge.h）放在仓库根 npb/ 目录，不作为本 crate 模块。

// ===== Phase 3 新增核心模块（思考/诊断链）=====
pub mod thinking_chain; // 存量+变量+补充增量=创新增量 的连续推演（思考链）
pub mod double_chain; // 问题形成过程 + 解决过程 的双链诊断

// ===== Phase 4 新增核心模块（化学变化层 + 场域解构）=====
pub mod evo_deconstructor; // 进化解构：时间三量 + 空间编码 → 层级1 胶粒
pub mod state; // 物态判定：能量态/气态/液态/固态（黄金阈值，调度核心）

// ===== 存在论/波动层新增模块（波粒二象性 + 感官 + 进化时间线）=====
pub mod evolution; // 进化过程记录与回放（步数为底层时间线）
pub mod fourier; // DFT 波动分析（主导频率/宽度/相位，π 为基础）
pub mod interference; // 干涉驻点检测（黄金层对齐 → 粒子生成）
pub mod senses; // 色声香味触法感官绑定与抽象

// ===== 痕迹层（习气与自我识别）=====
pub mod habit; // 习气累积（同类痕迹聚合，my_habits 识别自我习气）
pub mod self_recognizer; // 自我识别器（run→痕迹→习气→自我感；>0.7 触发）
pub mod trace; // 痕迹（风/火/水/地 + 指纹/存储）

/// 库当前版本（与 Cargo.toml 保持一致）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 一次 10000 步压力测试所用的迭代数（Phase 1 验收口径）。
pub const STRESS_ITERATIONS: u32 = 10_000;
