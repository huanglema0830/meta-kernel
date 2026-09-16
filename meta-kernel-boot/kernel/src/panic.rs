//! panic 处理（`no_std` bin 必需）。
//!
//! **设计口径**：本处**不试图刷屏**——
//! ① panic 时无法保证帧缓冲仍然可写（可能正是它的原因）；
//! ② 判定协议已足够：**只要屏幕不是绿，CI 就会红**（见 `verify` 的判定协议）。
//! 故此处只停机，**不制造"看起来通过"的假象**（C5/C7）。

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
