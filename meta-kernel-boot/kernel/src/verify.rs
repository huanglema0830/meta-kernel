//! # 元内核 · 裸机自检与帧缓冲输出（阶段二·子任务2.2）
//!
//! **路线 A（D31-d）：零 unsafe** ——
//! 屏幕输出只经 `bootloader_api` 的**安全 API**（`FrameBuffer::buffer_mut`），
//! 不使用端口 I/O、不触碰 `unsafe`（`kernel/src` 内 `unsafe` 出现次数须为 0）。
//!
//! ## 判定协议（供 QEMU 截图机读）
//!
//! 自检结果**编码为整屏颜色**：
//! - **绿** `#00FF00` ⇒ **全部自检通过**（含内存管理门禁 + 分配往返 + 数学/L4/戒律）
//! - **黄** `#FFFF00` ⇒ **内存门禁未通过**（拿不到 `physical_memory_offset` 或没有可用物理内存区）
//!   —— 即"**拿不到堆区就停**"，**不冒充成功**
//! - **红** `#FF0000` ⇒ 其它自检失败（具体编号见 [`self_check`]／`mem::alloc_roundtrip` 的错误）
//! - **不刷屏**（保持引导器输出） ⇒ 没有可用帧缓冲，无法判定
//!
//! 这样 CI 只需断言「截图中某像素 == 绿」，即可**同时**证明：
//! ① 引导器真的把内核加载并进入了入口；② **2.1 迁出的内核子集在裸机上真的算对了**；
//! ③ **2.3 的内存管理真的能用**（门禁通过 + 经 `GlobalAlloc` 的分配/释放/复用往返成立）。

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use bootloader_api::BootInfo;
use meta_kernel_core_nostd::{fmath, l4, l4_risk, quad::Quad};

/// 通过：绿
pub const COLOR_PASS: [u8; 3] = [0x00, 0xFF, 0x00];
/// 失败：红
pub const COLOR_FAIL: [u8; 3] = [0xFF, 0x00, 0x00];
/// **内存门禁未通过**：黄 —— "拿不到堆区就停"的专用信号（与"算法算错"区分开）
pub const COLOR_NO_HEAP: [u8; 3] = [0xFF, 0xFF, 0x00];

/// 判定结果（三态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 全过
    Pass,
    /// 内存门禁未过（拿不到堆区）——**不冒充成功**
    NoHeap,
    /// 其它自检失败，附编号
    Fail(u8),
}

/// 相对误差判据（f32）。
fn close(actual: f32, expect: f32, tol: f32) -> bool {
    let d = if actual > expect {
        actual - expect
    } else {
        expect - actual
    };
    d <= tol
}

/// 裸机自检。返回 `0` = 全过；非 0 = **第一个失败项的编号**（编号即失败原因，便于灰盒定位）。
///
/// 期望值一律写成**字面常量**（裸机无 `std`，不引外部数学库）：
/// √2 = 1.41421356、e = 2.71828183、ln(e) = 1、2¹⁰ = 1024。
pub fn self_check() -> u8 {
    // ① 自实现超越函数（2.1 的成果：7 个函数中的 4 个在此受检）
    if !close(fmath::sqrt(2.0), 1.414_213_5, 1e-5) {
        return 1;
    }
    if !close(fmath::exp(1.0), 2.718_281_7, 1e-5) {
        return 2;
    }
    if !close(fmath::ln(2.718_281_7), 1.0, 1e-4) {
        return 3;
    }
    if !close(fmath::powi(2.0, 10), 1024.0, 1e-2) {
        return 4;
    }

    // ② L4 戒律判据（阈值来源抽象层：由 Thresholds 注入，非直连参数库）
    let base = l4::dimension::FieldState::baseline();
    let benign = l4::dimension::FieldState::new(1.0, 1.05, 0.98, 1.0, 1.0, 1.0, 1.0);
    if l4::l4_gate::check_state(&benign, &base) != l4::l4_gate::Decision::Pass {
        return 5;
    }
    let far = l4::dimension::FieldState::new(2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);
    if l4::l4_gate::check_state(&far, &base) == l4::l4_gate::Decision::Pass {
        return 6;
    }

    // ③ 四戒律风险判定（含四元组挂接）
    let ok = l4_risk::RiskInput {
        name: "self-check",
        exposure: l4_risk::Exposure::SelfOnly,
        reversible: true,
        has_rollback: true,
        acknowledged_own_cause: true,
    };
    if l4_risk::assess(&ok, Quad::default()) != l4_risk::RiskVerdict::Cleared {
        return 7;
    }
    // 反向：触及他者 ⇒ **不害红线**，必须被拒（判据不能空转）
    let harm = l4_risk::RiskInput {
        name: "self-check",
        exposure: l4_risk::Exposure::Others,
        reversible: true,
        has_rollback: true,
        acknowledged_own_cause: true,
    };
    if l4_risk::assess(&harm, Quad::default())
        != l4_risk::RiskVerdict::Refused(l4_risk::Precept::NotHarm)
    {
        return 8;
    }

    0
}

/// 按判定结果刷整屏（**安全 API**；无帧缓冲时不动屏幕，交由 CI 判定"未出绿"即失败）。
pub fn render(boot_info: &mut BootInfo, verdict: Verdict) {
    let Some(fb) = boot_info.framebuffer.as_mut() else {
        return; // 无帧缓冲：保持引导器画面（CI 会因"非绿"而红，不会误判为通过）
    };
    let info = fb.info();
    let color = match verdict {
        Verdict::Pass => COLOR_PASS,
        Verdict::NoHeap => COLOR_NO_HEAP,
        Verdict::Fail(_) => COLOR_FAIL,
    };
    let buf = fb.buffer_mut();
    fill(buf, &info, color);
}

/// 单像素按 `PixelFormat` 写入（`p.len()` 即 `bytes_per_pixel`）。
fn write_pixel(p: &mut [u8], fmt: PixelFormat, rgb: [u8; 3]) {
    match fmt {
        PixelFormat::Rgb => {
            if p.len() >= 3 {
                p[0] = rgb[0];
                p[1] = rgb[1];
                p[2] = rgb[2];
            }
        }
        PixelFormat::Bgr => {
            if p.len() >= 3 {
                p[0] = rgb[2];
                p[1] = rgb[1];
                p[2] = rgb[0];
            }
        }
        PixelFormat::U8 => {
            if !p.is_empty() {
                p[0] = ((rgb[0] as u32 + rgb[1] as u32 + rgb[2] as u32) / 3) as u8;
            }
        }
        // 注意：PixelFormat 是 #[non_exhaustive]，兜底分支必须存在
        other => {
            if let PixelFormat::Unknown {
                red_position,
                green_position,
                blue_position,
            } = other
            {
                let v = ((rgb[0] as u32) << red_position)
                    | ((rgb[1] as u32) << green_position)
                    | ((rgb[2] as u32) << blue_position);
                for (i, byte) in p.iter_mut().enumerate() {
                    *byte = (v >> (i * 8)) as u8;
                }
            }
        }
    }
}

/// 刷满可视区域（按 `stride` 逐行推进；**边界全部显式判定**，避免越界 panic）。
fn fill(buf: &mut [u8], info: &FrameBufferInfo, rgb: [u8; 3]) {
    let bpp = info.bytes_per_pixel;
    if bpp == 0 || bpp > 8 {
        return;
    }
    let stride_bytes = info.stride.saturating_mul(bpp);
    let row_bytes = info.width.saturating_mul(bpp);
    if stride_bytes == 0 || row_bytes == 0 {
        return;
    }

    let rows = core::cmp::min(info.height, buf.len() / stride_bytes);
    for y in 0..rows {
        let base = y * stride_bytes;
        let mut col = 0usize;
        while col + bpp <= row_bytes {
            let start = base + col;
            let end = start + bpp;
            if end > buf.len() {
                break;
            }
            let (lo, hi) = (start, end);
            write_pixel(&mut buf[lo..hi], info.pixel_format, rgb);
            col += bpp;
        }
    }
}
