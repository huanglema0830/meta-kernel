//! # 唯一边界：`GlobalAlloc` 实现 + 物理内存切片（**本项目第一处真实 unsafe**）
//!
//! **C9 白名单登记项**：`meta-kernel-boot/kernel/src/mem/global.rs`（一条只登记一个文件）。
//! 本文件是**边界模块**，不是业务逻辑——所有算法都在 `meta-kernel-mem`（`#![forbid(unsafe_code)]`）。
//!
//! ## 本文件里的 unsafe 只有三类（逐块 `// SAFETY:` 论证）
//! 1. **把 bootloader 给的可用物理内存前段变成 `&'static mut [u8]`**（两张位图的存储）
//! 2. **访问 `static mut STATE`**（单线程内核；**前提：本阶段尚未开启中断**）
//! 3. **`unsafe impl GlobalAlloc`**（该 trait 本身是 unsafe trait）
//!
//! ## ⚠️ 两条必须写明的**边界条件**（改代码前先读）
//! - **非重入前提**：状态用 `static mut`，**没有锁**。当前成立的理由是
//!   **阶段二尚未启用中断/多核**；**一旦引入中断或 SMP，必须先加自旋锁**（届时属**新一次 C9 放行**）。
//! - **单帧上限**：`size > FRAME_SIZE`（4 KiB）的请求**返回 null**（不凑合）。
//!   支持"连续多帧"需要 `alloc_run(n)` 这一新原语（属后续增量，不在 2.3 范围内）。
//! - **禁 transmute**：本文件**不含** `transmute`（C9 明令）。

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use meta_kernel_mem::blocks::BlockAllocator;
use meta_kernel_mem::frames::FrameAllocator;
use meta_kernel_mem::{MemErr, Stats, BLOCK_SIZE, FRAME_SIZE};

/// 内核全局分配器（`main.rs` 用 `#[global_allocator]` 注册本类型的一个静态实例）。
pub struct KernelAlloc;

/// 注册用的常量实例（`main.rs` 以 `#[global_allocator]` 注册它构造的静态实例）。
///
/// **注意用 `const` 而不是 `static`**：`static` 不能被"移动"出去（会报 E0507），
/// 而 `const` 在使用处按值展开，正好符合零大小类型的用法。
pub const ALLOC: KernelAlloc = KernelAlloc;

/// 分配器状态：两张位图 + 两个竞技场。
struct HeapState {
    frames: FrameAllocator<'static>,
    blocks: BlockAllocator<'static>,
}

static mut STATE: Option<HeapState> = None;

/// 取状态的可变引用。
///
/// # Safety
/// - **单线程**：阶段二尚未启用中断/多核 ⇒ 不存在并发访问（见文件头"非重入前提"）。
///   引入中断/SMP 前**必须**改为加锁，否则本函数不再是 SAFETY 的。
/// - 调用方不得在持有该引用的同时再次调用本函数（无别名 `&mut`）。
// SAFETY: 本行声明为 `unsafe fn` —— 调用方必须满足"单线程 + 不并发持有 `&mut`"（函数级论证见其文档）。
unsafe fn state_mut() -> Option<&'static mut HeapState> {
    // SAFETY: 上述两条前提由调用方保证；`addr_of_mut!` 避免直接对 `static mut` 取引用
    //（edition 2024 的 `static_mut_refs` 限制），再用裸指针解引用得到 `&mut`。
    unsafe { (*ptr::addr_of_mut!(STATE)).as_mut() }
}

/// ★ **唯一的地址→切片转换点**：把元数据区前段做成两张位图。
///
/// # Safety
/// - `meta_virt..meta_virt+fb_len+bb_len` 必须是**已映射的可用物理内存**，
///   且**不与内核自身、bootloader 结构、栈重叠**（由 `mem::init` 从 `BootInfo` 的
///   `Usable` 区里划分，满足该条件）。
/// - 该区域在堆的**整个生命周期内**交给本分配器独占（不会被别处释放或复用）。
/// - 两张位图**不重叠**（`fb_len` 与 `bb_len` 是相邻两段）。
#[allow(clippy::too_many_arguments)]
pub fn init_arenas(
    meta_virt: usize,
    fb_len: usize,
    bb_len: usize,
    frames_virt: usize,
    blocks_virt: usize,
    frames_len: usize,
    blocks_len: usize,
) {
    // SAFETY: 见上方函数级 SAFETY——区域来自 BootInfo 的 Usable 区，且长度已由 `mem::init` 校验。
    let (fb_bytes, bb_bytes) = unsafe {
        let meta = ptr::slice_from_raw_parts_mut(meta_virt as *mut u8, fb_len + bb_len);
        let all: &'static mut [u8] = &mut *meta;
        all.split_at_mut(fb_len)
    };

    let frame_count = frames_len / FRAME_SIZE;
    // 帧竞技场的起点必须是帧对齐的（`mem::init` 已把 meta 长度上取整到帧）
    let base_frame = frames_virt / FRAME_SIZE;
    let block_count = blocks_len / BLOCK_SIZE;

    let frames = FrameAllocator::new(fb_bytes, frame_count, base_frame)
        .expect("帧位图容量已由 mem::init 校验");
    let blocks =
        BlockAllocator::new(bb_bytes, block_count, blocks_virt).expect("块位图容量已由 mem::init 校验");

    // SAFETY: 单线程（见 `state_mut` 的 SAFETY）；此处尚未向外界暴露任何 `&mut` 别名。
    unsafe {
        *ptr::addr_of_mut!(STATE) = Some(HeapState { frames, blocks });
    }
}

/// 当前统计（只读；无状态 ⇒ 非重入前提同样适用）。
pub fn stats() -> Stats {
    // SAFETY: 单线程；且本函数只读取统计，不做分配/释放，不会与调用方持有的 `&mut` 冲突
    //（调用方为内核自检，串行执行）。
    let st = unsafe { state_mut() };
    match st {
        Some(s) => {
            let fs = s.frames.stats();
            let bs = s.blocks.stats();
            Stats {
                frames_used: fs.frames_used,
                frames_total: fs.frames_total,
                blocks_used: bs.blocks_used,
                blocks_total: bs.blocks_total,
            }
        }
        None => Stats::default(),
    }
}

/// 经**真实 `GlobalAlloc` 接口**跑一次全链路：分配 → 写入 → 读回 → 释放 → **再分配复用**。
///
/// **为什么必须有它**：纯逻辑层 25 个测试证明的是"位图算法对"，
/// 而**边界层（本文件）的对错只能靠真正走一遍 `GlobalAlloc`** 才能证明。
pub fn roundtrip() -> Result<(), MemErr> {
    // 小块路径（≤ BLOCK_SIZE）
    const L1: Layout = Layout::new::<u64>();
    // SAFETY: `layout` 是编译期常量且 size/align 合法（8/8），满足 `GlobalAlloc::alloc` 的前置条件。
    let p1 = unsafe { ALLOC.alloc(L1) };
    if p1.is_null() {
        return Err(MemErr::BadCapacity);
    }
    // SAFETY: `p1` 由上面的 alloc 返回，是 L1 字节可写区且非空 ⇒ 可写一个 u64。
    unsafe { p1.cast::<u64>().write(0xA5A5_5A5A_1234_5678) };
    // SAFETY: 同上，指针有效且已初始化 ⇒ 可读回比较。
    let got = unsafe { p1.cast::<u64>().read() };
    if got != 0xA5A5_5A5A_1234_5678 {
        return Err(MemErr::AlreadyUsed);
    }
    // SAFETY: `p1` 来自同一 layout 的 alloc，尚未释放 ⇒ 可释放一次。
    unsafe { ALLOC.dealloc(p1, L1) };

    // 释放后应能复用同一地址（否则说明位图没真的清位）
    // SAFETY: 同上（layout 合法）。
    let p2 = unsafe { ALLOC.alloc(L1) };
    if p2 != p1 {
        // 先把它还回去，避免污染统计
        // SAFETY: `p2` 来自同一 layout 的 alloc，尚未释放。
        unsafe { ALLOC.dealloc(p2, L1) };
        return Err(MemErr::AlreadyFree);
    }
    // SAFETY: `p2` 来自同一 layout 的 alloc，尚未释放。
    unsafe { ALLOC.dealloc(p2, L1) };

    // 帧路径（> BLOCK_SIZE 且 ≤ FRAME_SIZE，且对齐 > BLOCK_SIZE 时也走帧）
    // SAFETY: 常量 layout，size=4096 页对齐 ⇒ 合法。
    let lf = unsafe { Layout::from_size_align_unchecked(FRAME_SIZE, FRAME_SIZE) };
    let pf = unsafe { ALLOC.alloc(lf) };
    if pf.is_null() {
        return Err(MemErr::BadCapacity);
    }
    if (pf as usize) % FRAME_SIZE != 0 {
        // SAFETY: `pf` 来自 lf 的 alloc，尚未释放。
        unsafe { ALLOC.dealloc(pf, lf) };
        return Err(MemErr::BadAlignment);
    }
    // SAFETY: `pf` 是 FRAME_SIZE 字节可写区 ⇒ 写首字节安全。
    unsafe { pf.write(0x5A) };
    // SAFETY: `pf` 来自 lf 的 alloc，尚未释放。
    unsafe { ALLOC.dealloc(pf, lf) };

    Ok(())
}

// SAFETY: `GlobalAlloc` 是 unsafe trait。安全前提（本实现负责保证）：
// - `alloc` 返回的指针指向**本分配器独占**的内存，且满足 `layout.align()` 与 `layout.size()`
//   （由 `meta-kernel-mem` 的纯逻辑保证：对齐经谓词筛选、块/帧粒度匹配）。
// - `dealloc` 收到的指针**必须**来自同一 layout 的 `alloc`（由调用方保证，`GlobalAlloc` 契约要求）。
// - 无并发访问（阶段二尚未启用中断/多核，见文件头"非重入前提"）。
unsafe impl GlobalAlloc for KernelAlloc {
    // SAFETY: `GlobalAlloc::alloc` 声明为 `unsafe fn`：实现必须"返回满足 layout 的内存，或返回 null"。
    // 本实现的分层策略由 `meta-kernel-mem`（零 unsafe、25 项 host 测试）保证对齐与粒度；
    // 不可满足的请求**返回 null 而不是凑合**（见文件头"单帧上限"）。
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: 单线程前提（见 state_mut 的 SAFETY）；本次调用不与其它 &mut 并存。
        let Some(st) = (unsafe { state_mut() }) else {
            return ptr::null_mut();
        };
        let size = layout.size();
        let align = layout.align();

        // 策略（与 `meta-kernel-mem` 的分层口径一致）：
        //   · size ≤ BLOCK_SIZE 且 align ≤ BLOCK_SIZE  ⇒ 小块
        //   · 否则 align ≤ FRAME_SIZE 且 size ≤ FRAME_SIZE ⇒ 帧
        //   · 其余（size > FRAME_SIZE 或 align > FRAME_SIZE）⇒ **拒绝**（返回 null，不凑合）
        if size == 0 {
            return ptr::null_mut();
        }
        if size <= BLOCK_SIZE && align <= BLOCK_SIZE {
            return match st.blocks.alloc(size, align) {
                Some(a) => a as *mut u8,
                None => ptr::null_mut(),
            };
        }
        if size <= FRAME_SIZE && align <= FRAME_SIZE {
            return match st.frames.alloc_aligned(align) {
                Some(f) => (f * FRAME_SIZE) as *mut u8,
                None => ptr::null_mut(),
            };
        }
        ptr::null_mut()
    }

    // SAFETY: `GlobalAlloc::dealloc` 声明为 `unsafe fn`：调用方须保证指针来自**同一 layout** 的 `alloc`。
    // 本实现按 layout 分流到小块/帧，且对"不该由本分配器返回的请求"**不做任何事**——
    // 避免误释放其它单元（静默破坏比报错更糟，故宁可不动）。
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        if p.is_null() {
            return;
        }
        // SAFETY: 单线程前提（见 state_mut 的 SAFETY）。
        let Some(st) = (unsafe { state_mut() }) else {
            return;
        };
        let size = layout.size();
        let align = layout.align();
        let addr = p as usize;

        if size <= BLOCK_SIZE && align <= BLOCK_SIZE {
            let _ = st.blocks.free(addr);
        } else if size <= FRAME_SIZE && align <= FRAME_SIZE {
            let _ = st.frames.free(addr / FRAME_SIZE);
        }
        // 其余情况本就不该由本分配器返回（alloc 已拒绝）⇒ 不做任何事（不静默破坏其它单元）
    }
}
