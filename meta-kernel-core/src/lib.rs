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

/// 库当前版本（与 Cargo.toml 保持一致）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 一次 10000 步压力测试所用的迭代数（Phase 1 验收口径）。
pub const STRESS_ITERATIONS: u32 = 10_000;
