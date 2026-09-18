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
//! **2.3b 迁移进度（2026-09-17 起）**：
//!
//! | 片 | 模块 | 状态 |
//! |---|---|---|
//! | 第 1 片 | [`l7::grade`] | ✅ **已迁入**（自包含、无 std、无浮点；仅补 1 行 `use alloc::vec::Vec`） |
//!
//! **仍不迁**：`l5_*`（baseline/quad/evidence）、`fourier`/`interference`/`energy`、
//! `l7::{ledger,mesh,repair}`（共 1,122 行中未迁部分）、
//! `gene_library`、`ontology`、`sanitizer`、`state`、`trace`、`habit`、`dna_*`、`executor` 等。
//!
//! ⚠️ **2.3b 的真实规模（订正）**：上轮清单写"10 文件 3,098 行"——那**只算了直接文件**。
//! 实测**非测试代码的依赖闭包 = 22 模块 / 6,853 行**（10 种子 3,098 + 12 个传递依赖 3,755），
//! 另有 **151 处 `std` 专有浮点方法**需改走 [`fmath`]。**故 2.3b 改为分片执行**，
//! 每片必须**自包含**（不引入未迁模块），逐片验证。清单见
//! `coordination/reports/2026-09-17_子任务2.3b_改动清单_订正.md`。
//!
//! **`alloc` 的地位**（**D34-b**）：`alloc` 属**标准分发**（随工具链提供、非第三方 crate），
//! **使用它不破 C1**；但使用时**不得因此引入任何外部 crate**。
//!
//! ## 测试策略
//!
//! `#![cfg_attr(not(test), no_std)]` —— 在 host 上跑 `cargo test` 时**启用 std**，
//! 于是 `fmath` 的精度测试可以直接与 `std::f32` 逐点对照（**CI 的 `test` job 覆盖**）；
//! 而 `cargo build --target x86_64-unknown-none` 时严格 `no_std`。
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

// **D34-b**：`alloc` 属**标准分发**（随工具链提供、非第三方 crate），使用它**不破 C1**。
// 裸机目标上 `alloc` 的可用性由 **`liballoc` 随 sysroot 提供 + boot 层注册 `#[global_allocator]`** 保证；
// 是否真的"能链接"由 **boot 层的 `alloc` 探针 + QEMU 绿屏**证成（不靠宣称）。
extern crate alloc;

pub mod math;

pub mod fmath;
pub mod quad;

pub mod expo;
pub mod fib;
pub mod linear;

pub mod l4;
pub mod l4_risk;

pub mod l7;

// —— 2.3b 片2（2026-09-17）：三个自包含模块（详见各文件头"迁移说明"） ——
pub mod fourier;
pub mod l5_baseline;
pub mod l5_senses;

// —— 2.3b 片3（2026-09-17）：元标尺链（片内自包含；依赖 `math` 与片2 的 `fourier`） ——
//   ⚠️ 本片含**两处类型替换**（`HashSet` 在 no_std 不存在）：
//   `ontology` 的 `std::collections::HashSet` → `alloc::collections::BTreeSet`
//   —— 该集合**只用 insert/len、从不迭代** ⇒ 逐位等价（编译级证据与论证见报告 D37）。
pub mod ontology;
pub mod state;
pub mod energy;
pub mod sanitizer;
pub mod interference;

// —— 2.3b 片4（2026-09-17）：世界/语境/四元组链（**修正版 9 模块**）——
//   ⚠️ **为什么是 9 个而不是原清单的 5 个**：原清单的 5 模块
//   （`l1_field_parse`/`l3_world`/`l5_quad`/`l5_context`/`l5_evidence`）**不是封闭集** ——
//   它们**反向依赖** `trace`/`dna_generate`/`dna_trace`/`gene_library`（且 `gene_library` 与
//   `l5_context` **互引成环**）。Rust **同 crate 内模块可互引、无需拓扑序**，
//   真正的约束是「**迁移集必须在 nostd 内封闭**」⇒ 取**最小封闭超集**（9 模块 / 3,681 行）。
//   详见报告 §三 与 `ROADMAP.md`（分片方案已被修正为「闭包」而非「链式」）。
pub mod trace;
pub mod dna_generate;
pub mod dna_trace;
pub mod habit;
pub mod gene_library;
pub mod l5_context;
pub mod l1_field_parse;
pub mod l3_world;
pub mod l5_quad;
pub mod l5_evidence;
// —— 2.3b 片5（2026-09-17）：**清单由脚本产出**（D40）——
// 生成命令：`python coordination/tools/check_migration_closure.py --plan`
// 取「层 1」＝所依赖的未迁模块为空的 16 模块 / 3,651 行（含 `#[cfg(test)]` 依赖一并计入）。
pub mod l1_source_parse;
pub mod thinking_chain;
pub mod hourglass;
pub mod evolution;
pub mod l5_attention;
pub mod gate;
pub mod self_recognizer;
pub mod dna_adapt;
pub mod double_chain;
pub mod mirror;
pub mod senses;
pub mod evo_deconstructor;
pub mod persist;
pub mod executor;
pub mod l5_compare;
// —— 2.3b 片6（2026-09-18）：**清单由脚本产出**（D40），冻结串＝`--emit 1` 的输出 ——
// 生成命令：`python coordination/tools/check_migration_closure.py --emit 1`
// ⇒ `l1_mapping l5_diagnosis l7/repair positive_source`（4 模块 / 2,337 行，脚本口径）
// ⚠️ **本片含两处「替换类」**：
//   ① 唯一的**类型替换**：`positive_source` 的 `std::collections::HashMap` → `alloc::collections::BTreeMap`
//      —— 成立前提＝该集合**只用 insert/get、从不迭代**（方法集恰为 {insert, get}；
//         derive 为 `Debug, Clone, Default`，无 `PartialEq`/`Eq`/`Hash`）⇒ 遍历序不可观测。
//      ⚠️ 是**行为（内容）等价**，**不是"逐位等价"**（措辞已订正）。
//   ② **唯一一处「改文字」**：同文件 **3 行 `O(1)` 注释 → `O(log n)`**（行 35／337／409），
//      因 `BTreeMap::get` 是 `O(log n)`。**已显式登记在 `check_migration_fidelity.py::ALLOWED_SUBS`**
//      作为**可追溯项**（旧判据只证"没多改字"，**不证"改了的那字对"** ⇒ 这是它的盲区）。
//      ⚠️ **std 侧 `meta-kernel-core` 保持 `HashMap` 不变** ⇒ 那边 `O(1)` 仍为真，替换是**单向的**。
pub mod l1_mapping;
pub mod l5_diagnosis;
pub mod positive_source;
