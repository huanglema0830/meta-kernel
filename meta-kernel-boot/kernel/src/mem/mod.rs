//! # 内存管理接线（阶段二·子任务2.3）
//!
//! **本文件是 boot 层的"策略层"**：决定"从哪块物理内存取堆"，然后交给
//! [`meta_kernel_mem`]（纯逻辑、零 unsafe）与 [`global`]（唯一边界）去干活。
//!
//! ## 门禁（用户 2026-09-17 明确要求：**拿不到堆区就停，不得绕过**）
//!
//! [`init`] 的前两步就是门禁，**任一不满足即返回 `Err`**：
//! 1. **必须能拿到 `physical_memory_offset`** —— 它只在 bootloader 配置了
//!    `mappings.physical_memory = Some(Mapping::Dynamic)` 时才存在（见 `main.rs` 的 `BOOTLOADER_CONFIG`）；
//! 2. **必须存在 `Usable` 内存区**，且足以容纳元数据 + 两块竞技场。
//!
//! 门禁失败 ⇒ 内核把屏幕刷成**黄**（判定协议见 `verify.rs`）**而不是绿** ⇒ CI 的像素断言**必然红**。
//! 也就是说：**"拿不到堆区却宣告成功"在物理上不可能发生**。
//!
//! ## 判定编号（`selftest` 返回值；**勿随意改号**，截图即靠它定位）
//!
//! | 编号 | 含义 |
//! |---|---|
//! | 1 | 索引越界 |
//! | 2 | 重复释放 |
//! | 3 | 重复占用 |
//! | 4 | 容量不足（含"元数据放不下"） |
//! | 5 | 对齐不满足 |
//! | **6** | **堆几何不合理**（竞技场过小 / 区无效 / 容量为 0） |
//! | **7** | **初始化后统计非净零**（新堆不该有占用） |
//! | **8** | **往返后统计非净零**（⇒ **存在泄漏**） |
//! | **9** | **`alloc::vec::Vec` 内容不符**（`alloc` 探针失败） |
//! | **10** | **`alloc::string::String` 内容不符**（`alloc` 探针失败） |
//! | **11** | **`alloc` 探针后统计非净零**（⇒ `Vec`/`String` 泄漏） |

use alloc::string::String;
use alloc::vec::Vec;
use bootloader_api::info::MemoryRegionKind;
use bootloader_api::BootInfo;
use meta_kernel_mem::{MemErr, Stats, BLOCK_SIZE, FRAME_SIZE};

pub mod global;

/// 元数据（两张位图）占总区的比例分母：`len/64`（≈1.6%）。
const META_DIV: usize = 64;

/// 堆布局结果（供几何自检使用）。
pub struct Heap {
    /// 使用的物理内存区 `[phys_start, phys_end)`
    pub region: (u64, u64),
    /// 元数据字节数
    pub meta_len: usize,
    /// 帧竞技场字节数
    pub frames_len: usize,
    /// 小块竞技场字节数
    pub blocks_len: usize,
}

/// 挑出**最大**的 `Usable` 区（起点、终点，物理地址）。
fn largest_usable(boot_info: &BootInfo) -> Option<(u64, u64)> {
    boot_info
        .memory_regions
        .iter()
        .filter(|r| r.kind == MemoryRegionKind::Usable)
        .map(|r| (r.start, r.end))
        .filter(|(s, e)| e > s)
        .max_by_key(|(s, e)| e - s)
}

/// 把 `MemErr` 映射成**稳定判定编号**（1–5）。
fn code_of(e: MemErr) -> u8 {
    match e {
        MemErr::OutOfRange => 1,
        MemErr::AlreadyFree => 2,
        MemErr::AlreadyUsed => 3,
        MemErr::BadCapacity => 4,
        MemErr::BadAlignment => 5,
    }
}

/// 建立堆。**这是门禁**：任一步不满足即 `Err`。
pub fn init(boot_info: &mut BootInfo) -> Result<Heap, MemErr> {
    // —— 门禁①：物理内存映射必须可用 ——
    let phys_off = boot_info
        .physical_memory_offset
        .into_option()
        .ok_or(MemErr::OutOfRange)?;

    // —— 门禁②：必须存在可用的物理内存区 ——
    let (start, end) = largest_usable(boot_info).ok_or(MemErr::OutOfRange)?;
    let len = (end - start) as usize;
    if len < FRAME_SIZE * 4 {
        return Err(MemErr::BadCapacity);
    }

    // —— 布局： [元数据位图][帧竞技场][小块竞技场] ——
    let meta_raw = len / META_DIV;
    // 元数据须整帧对齐，才能让帧竞技场从帧边界开始
    let meta_len = meta_raw.div_ceil(FRAME_SIZE) * FRAME_SIZE;
    let rest = len.checked_sub(meta_len).ok_or(MemErr::BadCapacity)?;
    let frames_len = rest / 2;
    let blocks_len = rest - frames_len;
    if meta_len == 0 || frames_len < FRAME_SIZE || blocks_len < BLOCK_SIZE {
        return Err(MemErr::BadCapacity);
    }

    let frame_count = frames_len / FRAME_SIZE;
    let block_count = blocks_len / BLOCK_SIZE;
    let fb_len = frame_count.div_ceil(8);
    let bb_len = block_count.div_ceil(8);
    if fb_len + bb_len > meta_len {
        return Err(MemErr::BadCapacity); // 元数据放不下 ⇒ 不做"勉强能用"的凑合
    }

    // 地址换算：物理 → 虚拟（`phys_off` 由 bootloader 提供）
    let meta_virt = (start as usize) + phys_off as usize;
    let frames_phys = start + meta_len as u64;
    let frames_virt = (frames_phys as usize) + phys_off as usize;
    let blocks_virt = frames_virt + frames_len;

    // ★ 唯一边界：把两段物理内存变成切片（unsafe 本体在 `global.rs`，此处只调用它）
    global::init_arenas(
        meta_virt, fb_len, bb_len, frames_virt, blocks_virt, frames_len, blocks_len,
    );

    Ok(Heap {
        region: (start, end),
        meta_len,
        frames_len,
        blocks_len,
    })
}

/// 取当前统计（供自检）。
pub fn stats() -> Stats {
    global::stats()
}

/// 自检失败的**两类**原因（与判定颜色一一对应，**勿混**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemFail {
    /// **堆区不可用**（拿不到物理内存映射 / 无可用区 / 容量不足）⇒ **黄屏**
    /// ——这正是用户要求的"拿不到堆区就停"。
    Unavailable,
    /// **分配器行为错**（编号见文件头）⇒ **红屏**。
    Bug(u8),
}

/// **内存管理自检**（返回 `Ok(())` = 全过）。
///
/// 断言链（**每一步都可能失败，故都不是空转**）：
/// 1. `init` 门禁通过
/// 2. **几何合理**：竞技场不小于一个单元、区有效、容量非零
/// 3. **初始化后净零**：新堆不该有任何占用
/// 4. **往返**：经真实 `GlobalAlloc` 做 分配→写入→读回→释放→**再分配复用**，并覆盖帧路径对齐
/// 5. **`alloc` 探针**（2.3b 门禁）：`Vec`/`String` 经真实 `GlobalAlloc` 走一遍
/// 6. **净零**：往返 + 探针后占用回到 0 且容量未被改变 ⇒ **无泄漏**
pub fn selftest(boot_info: &mut BootInfo) -> Result<(), MemFail> {
    let heap = match init(boot_info) {
        Ok(h) => h,
        // 门禁不满足 ⇒ 归"堆区不可用"（环境/配置问题），**不是**分配器逻辑错
        Err(MemErr::OutOfRange) | Err(MemErr::BadCapacity) => return Err(MemFail::Unavailable),
        Err(e) => return Err(MemFail::Bug(code_of(e))),
    };

    // ② 几何
    if heap.region.1 <= heap.region.0
        || heap.meta_len == 0
        || heap.frames_len < FRAME_SIZE
        || heap.blocks_len < BLOCK_SIZE
    {
        return Err(MemFail::Unavailable);
    }

    // ③ 初始化后净零
    let before = stats();
    if before.frames_total == 0 || before.blocks_total == 0 {
        return Err(MemFail::Unavailable);
    }
    if before.frames_used != 0 || before.blocks_used != 0 {
        return Err(MemFail::Bug(7));
    }

    // ④ 往返
    global::roundtrip().map_err(|e| MemFail::Bug(code_of(e)))?;

    // ④b ★ `alloc` 探针（2.3b 门禁）
    alloc_probe()?;

    // ⑤ 往返 + 探针后净零（**无泄漏**）
    let after = stats();
    if after.frames_used != 0 || after.blocks_used != 0 {
        return Err(MemFail::Bug(8));
    }
    if after.frames_total != before.frames_total || after.blocks_total != before.blocks_total {
        return Err(MemFail::Bug(6));
    }

    Ok(())
}

/// ★ **`alloc` 探针** —— 2.3b 的门禁：`alloc` 能否在**裸机目标**上链接并运行。
///
/// **为什么单独设这一步**：`extern crate alloc;` 若**没有任何代码真正使用**，
/// 链接器根本不会去解析 `liballoc` —— 于是"用了 alloc"这件事**从未被验证**，
/// 只会得到一个"看起来能编"的假象（＝判断空转）。本探针逼 `Vec`/`String` 
/// **真实走一遍 `GlobalAlloc`**，其释放由外层的**净零断言**兜底（⇒ 顺带证**无泄漏**）。
///
/// 覆盖三种不同分配路径：`Vec` 渐进增长（alloc→realloc）／`with_capacity`（大块）／
/// `String` 拼接与 `format!`（fmt 机制）。
fn alloc_probe() -> Result<(), MemFail> {
    // ① Vec 渐进增长（每步可能触发 realloc）
    let mut v: Vec<u32> = Vec::new();
    for i in 0..64u32 {
        v.push(i.wrapping_mul(3));
    }
    let want: u32 = (0..64u32).map(|i| i.wrapping_mul(3)).sum();
    if v.len() != 64 || v[0] != 0 || v[63] != 189 || v.iter().sum::<u32>() != want {
        return Err(MemFail::Bug(9));
    }

    // ② with_capacity + resize（另一条 layout 路径）
    let mut w: Vec<u8> = Vec::with_capacity(96);
    w.resize(96, 0x5A);
    if w.len() != 96 || w.iter().any(|b| *b != 0x5A) {
        return Err(MemFail::Bug(9));
    }

    // ③ String 拼接 + format!（fmt 机制）
    let mut s = String::with_capacity(8);
    s.push_str("meta-kernel");
    s.push('/');
    s.push_str("boot");
    if s.as_str() != "meta-kernel/boot" || s.len() != 14 {
        return Err(MemFail::Bug(10));
    }
    let t = alloc::format!("{}-{}", s.len(), v.len());
    if t.as_str() != "14-64" {
        return Err(MemFail::Bug(10));
    }

    // ④ 全部释放 —— 净零由外层 ⑤ 断言
    drop(t);
    drop(s);
    drop(w);
    drop(v);
    Ok(())
}
