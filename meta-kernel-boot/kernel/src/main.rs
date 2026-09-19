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

/// 自检与门禁。**顺序不可调换**：内存门禁在算法自检之前；⑫ 段（帧缓冲写入）在算法自检之前。
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
    // —— 算法自检（2.1 成果：fmath + L4 判据 + 四戒律风险） ——
    match verify::self_check() {
        0 => Verdict::Pass,
        n => Verdict::Fail(100 + n), // 100+ 区段 = 算法层失败（与内存编号区分）
    }
}
