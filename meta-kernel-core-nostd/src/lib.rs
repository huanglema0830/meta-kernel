//! # 元内核 · `no_std` 子集（阶段二 · 子任务2.1 · 路径 A-2）
//!
//! 目标：把内核的**纯计算部分**搬到 `x86_64-unknown-none`（裸机），
//! 为后续 **GRUB/Limine 引导（2.2）→ 最简内存（2.3）→ DRM/KMS 直渲（2.4）** 提供计算底座。
//!
//! ## 与 `meta-kernel-core` 的关系（**只新增，不修改**）
//!
//! - 原 `meta-kernel-core` **完整保留**（宿主 / 网关 / 工作台仍在用 std 版）。
//! - 本 crate 是它的**子集 + 拆分版**：
//!   **凡"序列化 / 文件 IO / 基因库读写"的接口一律留在 std 侧**，本层只保留**纯计算**。
//! - **零第三方依赖**（C1）；**不使用 `alloc`**（本层不出现 `Vec` / `String`）。
//!
//! ## 阈值来源抽象（本次拆分的核心）
//!
//! ```text
//! std 侧：   GeneLibrary ──from_library()──> Thresholds（纯数据）
//! no_std 侧：Thresholds ──> L4 判据（dimension / l4_gate / l4_router）
//! ⇒ 本层**完全不出现 `GeneLibrary` 符号**
//! ```
//!
//! ## 模块
//!
//! | 模块 | 内容 | 来源 |
//! |---|---|---|
//! | [`math`] | 饱和加法 / `clamp01` / `is_valid` | 复制自 core（零改造） |
//! | [`fmath`] | **自实现**浮点超越函数（`sqrt`/`exp`/`ln`/`sin`/`cos`/`atan2`/`powi`）—— core 不提供 | **全新** |
//! | [`quad`] | `Quad` 四元组**数据契约**（纯数据 + 无 alloc 方法；序列化留 std） | **抽取**（同步要求见文件头） |
//! | [`linear`] [`expo`] [`fib`] | 线性 / 指数 / 斐波那契变化模式 | 复制（零改造） |
//! | [`l4`] | 拒绝层：七维场域判据 + 路由（阈值经 `Thresholds` 注入） | 复制 + **拆分** |
//! | [`l4_risk`] | 四条戒律风险判定（**纯判据**；文本输出留 std） | 复制 + **拆分** |
//!
//! **不迁（留 2.3）**：`l5_*`（baseline/quad/evidence）、`fourier`/`interference`/`energy`、
//! **`l7` 及其 4 个子模块（`grade`/`ledger`/`mesh`/`repair`，共 1,122 行，均含 alloc）**、
//! `gene_library`、`ontology`、`sanitizer`、`state`、`trace`、`habit`、`dna_*`、`executor` 等。
//! 判据见报告：**"alloc 出现在算法内部/结构体字段" ⇒ 本轮不强拆**（避免改算法语义）。
//!
//! ## 测试策略
//!
//! `#![cfg_attr(not(test), no_std)]` —— 在 host 上跑 `cargo test` 时**启用 std**，
//! 于是 `fmath` 的精度测试可以直接与 `std::f32` 逐点对照（**CI 的 `test` job 覆盖**）；
//! 而 `cargo build --target x86_64-unknown-none` 时严格 `no_std`。
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

pub mod math;

pub mod fmath;
pub mod quad;

pub mod expo;
pub mod fib;
pub mod linear;

pub mod l4;
pub mod l4_risk;
