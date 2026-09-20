//! # 元内核 · 裸机内核入口（阶段二·子任务2.2 / 2.3）
//!
//! 由 `bootloader` 0.11 加载。入口经 `entry_point!` 宏注册（生成 `_start`）。
//!
//! 依赖边界（**D31-a 已确认**）：`bootloader_api` 只出现在**本 boot 层**；
//! `meta-kernel-core` 与 `meta-kernel-core-nostd` **保持零依赖**（C1 未松动）。
//!
//! **2.3 新增**：内存管理。执行顺序即**门禁顺序**——
//! ① 开 `map-physical-memory` → ② `mem::init`（拿不到堆区 ⇒ **黄屏停**）
//! → ③ `mem::alloc_roundtrip`（经 `GlobalAlloc` 的真实往返）→ ④ 数学/L4/戒律自检 → ⑤ 刷屏。

#![no_std]
#![no_main]
// 刻意不用 #![forbid(unsafe_code)]：`entry_point!` 宏展开可能内含 unsafe。
// 本层 unsafe 面的判据＝**C9 门禁**（白名单 + SAFETY 计数 + 禁 transmute）；
// 而**内存管理的算法本体在 `meta-kernel-mem`（那边有 `#![forbid(unsafe_code)]`）**。

extern crate alloc;

mod mem;
mod panic;
// 2.4 **边界层**：帧缓冲写入（`O-1` 已同意，2026-09-20 开工）。见 `present.rs` 头部的落点判据。
mod present;
mod verify;

use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::{entry_point, BootInfo};
use meta_kernel_core_nostd::energy::EnergyPool;
use meta_kernel_core_nostd::field;
use verify::Verdict;

/// ⚠️ **内存门禁的前提**：不开这个映射，`BootInfo.physical_memory_offset` 就是 `None`，
/// 内核**无法访问**任何可用物理内存 ⇒ 分配器无从建立（内核会刷**黄屏**并停）。
pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut c = BootloaderConfig::new_default();
    c.mappings.physical_memory = Some(Mapping::Dynamic);
    c
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

/// 注册全局分配器（**唯一边界在 `mem/global.rs`**；C9 白名单登记项）。
#[global_allocator]
static GLOBAL: mem::global::KernelAlloc = mem::global::ALLOC;

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let verdict = run(boot_info);
    verify::render(boot_info, verdict);
    loop {
        core::hint::spin_loop();
    }
}

/// 自检与门禁。**顺序不可调换**：内存门禁在算法自检之前；⑫ 段（帧缓冲写入：**自检路径 121–125
/// ＋ product 路径 126–128 ＋ 场演化路径 129–132**）在算法自检之前。
fn run(boot_info: &mut BootInfo) -> Verdict {
    // —— 内存门禁 + 分配往返 + 净零校验（拿不到堆区 ⇒ 黄屏停，不冒充成功） ——
    match mem::selftest(boot_info) {
        Ok(()) => {}
        Err(mem::MemFail::Unavailable) => return Verdict::NoHeap,
        Err(mem::MemFail::Bug(code)) => return Verdict::Fail(code),
    }
    // —— ⑫ 段：**2.4 边界层接入帧缓冲**（真实写入 → 回读 → 逐字节比对）——
    // 为什么放在最后刷屏之前：`verify::render()` 会 `fill()` **整屏** ⇒ 本段写入的 2×2 图案
    // **必然被覆盖** ⇒ **不改变绿/红判定**（既有门禁的语义零变化），但**写入路径被真实走过一遍**。
    // 编号：121–125；经下面的 `100 + n` 映射后，CI 上读作 **221–225**（与 ⑪ 段的 211–215 沿用同一约定）。
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buf = fb.buffer_mut();
        let r = present::present_selfcheck(buf, &info);
        if r != 0 {
            return Verdict::Fail(100 + r);
        }
    }
    // —— ⑫ 段（**product 路径**）：**真的调用 `present_field`**（2.4 产品入口接线，2026-09-20）——
    // 为什么单列：上面那条走的是**写死的 2×2 图案**，**不经过产品入口** ⇒ 若不在此真调
    // `present_field`，产品路径在裸机上**从未被走过**（编译期 `never used` 警告即是证据）；
    // "漏挂 = 静默空转" —— 绿屏照样绿，却证不了产品路径可用。
    // 场源＝纯算层 `field::sdf` 依**帧缓冲几何**采样（D8：呈现＝内核状态的直接投影），并**回读校验**。
    // 同样被 `render()` 的整屏 `fill()` 覆盖 ⇒ **不改变绿/红判定**。
    // 编号：126–128；经 `100 + n` 映射后，CI 上读作 **226–228**。
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buf = fb.buffer_mut();
        let r = present::present_product_selftest(buf, &info);
        if r != 0 {
            return Verdict::Fail(100 + r);
        }
    }
    // —— ⑫ 段（**场演化路径**）：**B 路径「低维场演化产生图像」的小范围验证**（2026-09-20）——
    // 与上一条的区别：那条的场是**静态**的（SDF 一帧），本条的场是**多步演化**出来的
    // ⇒ 验的是"**场演化 → 投影 → 像素**"这条链**真的通**（**恒等映射骗不过去**：步数必须真走完）。
    // **步数从哪来**：`field::rhythm_steps` —— 它 **import** `engine_select`（Q11 口径，**不重实现**，C19）
    // 把"物态 → 引擎"映射成"本轮推进几步"，且 `steps ∈ [1, base+1]` ⇒ **永不为 0**
    // （否则"不推进"会让判据**假绿**）。
    // 同样被 `render()` 的整屏 `fill()` 覆盖 ⇒ **不改变绿/红判定**。
    // 编号：129–132；经 `100 + n` 映射后，CI 上读作 **229–232**。
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buf = fb.buffer_mut();
        // 节律：物态 → 引擎 → 步数（**纯算层**，不改场）。此处取 Liquid 物态（与 Q11 契约同款取值）。
        let pool = EnergyPool { flow_in: 0.9, flow_out: 1.0, stored: 1.0 };
        let (_engine, steps) = field::rhythm_steps(&pool, 0.5, 3);
        let r = present::present_evolve_selftest(buf, &info, steps);
        if r != 0 {
            return Verdict::Fail(100 + r);
        }
    }
    // —— 算法自检（2.1 成果：fmath + L4 判据 + 四戒律风险） ——
    match verify::self_check() {
        0 => Verdict::Pass,
        n => Verdict::Fail(100 + n), // 100+ 区段 = 算法层失败（与内存编号区分）
    }
}
