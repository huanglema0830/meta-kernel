//! # npb-appkit — L4 应用框架（点亮→外显）实现
//!
//! 依据：docs/L4_APPLICATION_FRAMEWORK_DESIGN v1.0（发起人审核通过，2026-09-08）
//! 层位：L4 · 框架·拒绝层（LAYER_ARCHITECTURE L0-L6）
//! 戒律：拒绝层——拒绝无溯源表达、拒绝越权造内核事实、拒绝低阈值抖动（防抖）。
//!
//! 模块：
//! - [`lifecycle`]：显化生命周期状态机（0 ∪ {10..=99}，纯函数，N=8 回融窗、跨带双事件防抖）
//! - [`namer`]：命名 registry（双轨：内部符号 ↔ 外部名；条目预留 `twin` 字段位）
//! - [`speaker`]：语言组织（指称→陈述→意图三层，输出必可溯源 source）
//! - [`event_pipe`]：L3 SSE 订阅 → 归一 [`KernelEvent`]（事件驱动，不空转）
//! - [`httpc`]：极简 HTTP 客户端基元（POST push / SSE 读取；std，与 npb-gateway 服务器对称）
//!
//! 设计铁律：本 crate 不依赖 npb/npb-gateway 运行面（纯 std）；L4 只消费 L3 已发布协议；
//! 生命周期是应用侧仪表，不是内核字段——所有输入来自网关事件，来源字段全程携带（可溯源）。

pub mod event_pipe;
pub mod httpc;
pub mod lifecycle;
pub mod namer;
pub mod speaker;

pub use event_pipe::{EventPipe, RawEvent};
pub use lifecycle::{KernelEvent, LifecycleEngine};
pub use namer::{NamedEntity, Namer};
pub use speaker::{Intent, Speaker, Statement};

/// 意图原语常量（一期基础原语）。
pub const INTENT_SUGGEST_NEXT: &str = "suggest_next";
pub const INTENT_JOURNAL: &str = "journal";
