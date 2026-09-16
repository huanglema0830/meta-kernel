//! # 元内核 · 裸机内核入口（阶段二·子任务2.2）
//!
//! 由 `bootloader` 0.11 加载。入口经 `entry_point!` 宏注册（生成 `_start`）。
//!
//! 依赖边界（**D31-a 已确认**）：`bootloader_api` 只出现在**本 boot 层**；
//! `meta-kernel-core` 与 `meta-kernel-core-nostd` **保持零依赖**（C1 未松动）。

#![no_std]
#![no_main]
// 刻意不用 #![forbid(unsafe_code)]：entry_point! 宏展开可能内含 unsafe，
// 本层"零 unsafe"的判据改为**对 kernel/src 做 grep 计数**（见报告 边界说明）。

mod panic;
mod verify;

use bootloader_api::{entry_point, BootInfo};

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // ① 自检（0 = 全过）
    let report = verify::self_check();
    // ② 把结果刷成整屏颜色，供 QEMU 截屏机读
    verify::render(boot_info, report);
    // ③ 停机——**不返回**（QEMU 由 CI 侧超时后截屏并退出）
    loop {
        core::hint::spin_loop();
    }
}
