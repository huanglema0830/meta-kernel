//! # 最简内存管理（阶段二·子任务2.3）
//!
//! **分层（对齐 C9「边界形态」）**：
//!
//! | 层 | 在哪 | unsafe |
//! |---|---|---|
//! | **纯逻辑**（本 crate：位图 / 帧 / 小块 的分配与释放） | `meta-kernel-mem` | **0（`#![forbid(unsafe_code)]` ⇒ 结构性保证）** |
//! | **唯一边界**（`GlobalAlloc` 实现 + 物理内存切片） | boot 层 `mem/global.rs` | 有（**C9 白名单登记，逐块 `// SAFETY:`**） |
//!
//! **为什么这样分**：算法本体是"位图 + first-fit"，**可以用普通 Rust 写、也就能在 host 上跑测试**；
//! 真正危险的只有两件事——"把物理地址变成切片"与"实现 `unsafe trait GlobalAlloc`"——
//! 把它们**isolate 到一个文件**，就得到一个**可测、可审计**的最简分配器。
//!
//! **不使用 `alloc`**：本 crate 只操作调用方传进来的缓冲区，因此不需要分配器，**也没有循环依赖**。
//!
//! ## 与 C1 / C9 的关系
//! - **C1**：零第三方依赖（只用 `core`）
//! - **C9**：本 crate 通过 `forbid(unsafe_code)` **机械地保证**"算法层不碰 unsafe"
//! - 本 crate **不做**任何 IO、不碰硬件、不认识物理地址（只认**索引**）——地址换算留给边界层

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

pub mod bitmap;
pub mod blocks;
pub mod frames;

/// 物理帧大小（4 KiB，x86_64 常规页）。
pub const FRAME_SIZE: usize = 4096;

/// 小块分配粒度（≤4 KiB 的请求走小块；更大/更强的对齐走帧）。
pub const BLOCK_SIZE: usize = 64;

/// 内存管理错误（**全部是可判定的"用错"**，不含"内存不足"——那由 `Option` 表达）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemErr {
    /// 索引超出容量
    OutOfRange,
    /// 释放了一个本来就空闲的单元（**重复释放**）
    AlreadyFree,
    /// 再次占用一个已被占用的单元
    AlreadyUsed,
    /// 容量参数与缓冲区不匹配（如 `bits > bytes.len()*8`）
    BadCapacity,
    /// 对齐要求无法由本层满足（例如 align > FRAME_SIZE）
    BadAlignment,
}

/// 分配统计（供自检与判定信号使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// 已占用的**帧**数
    pub frames_used: usize,
    /// 帧总容量
    pub frames_total: usize,
    /// 已占用的**小块**数
    pub blocks_used: usize,
    /// 小块总容量
    pub blocks_total: usize,
}

impl Stats {
    /// 是否"有容量且尚未被用满"（用于自检：**不是**恒真条件）。
    pub fn has_capacity(&self) -> bool {
        self.frames_total > 0 && self.blocks_total > 0
    }
}
