//! # 2.4 **边界层**：帧缓冲写入（`present`）—— O-1 已同意，2026-09-20 开工
//!
//! ## 一句话
//! **本文件是"纯算层算好的像素"跨到"真实硬件内存"的唯一一道门。**
//!
//! ## 为什么它必须在 boot 层（**落点判据，非感觉**）
//! | 铁律 | 推出什么 |
//! |---|---|
//! | **C1**（内核零依赖） | `bootloader_api` **只许在 boot 层出现** ⇒ 桥接函数（`FrameBufferInfo` → `FbDesc`）**只能在这里**；纯算层不得 `use` 它 |
//! | **机制 21**（执行体边界） | "算像素"＝纯算（在 `nostd::fb`／`nostd::project`）；"**持有并改写硬件内存**"＝边界 ⇒ **本文件** |
//! | **C9**（`unsafe` 边界） | 全程走 `bootloader_api` 的**安全 API**（`buffer_mut()`）＋ 切片写入 ⇒ **本文件 0 处 `unsafe`** |
//!
//! ## 与 `verify.rs` 的关系（**重构，不是重写**）
//! `verify.rs::write_pixel()`（约行 899–937）**已内含**编码逻辑（把"编码"与"写入"写在一起）。
//! 本文件改用 `nostd::fb` 的 [`fb::put_pixel`]／[`fb::encode`]（**同一份逻辑，只此一处实现**，C19 同源），
//! `verify.rs` 的旧函数**本轮不动**（"只新增不删除"）—— 两者**必须给出相同字节**，
//! 这条等价性由 `tests/mirror_bare_assertions.rs` 的 `mirror_present_*` 在 host 上断言。
//!
//! ## 本文件**不做**什么（显式排除，防越权）
//! - ❌ **不碰** MMIO／DRM／KMS／GPU 调度／显存管理（2.4 主线定义＝"轻量帧缓冲 ＋ L6 接入"）。
//! - ❌ **不做双缓冲**（当前分配器单帧上限 4 KiB，1280×720×3 ＝ 2.64 MiB，给不出来）。
//! - ❌ **不做任何"美化"**（D8：呈现＝内核状态的直接投影；无 gamma／无锐化／无调色）。
//!
//! ## ⚠️ 诚实标注（C5：本机不可验证）
//! **本机无 MSVC 库／`clang`／`lld` ⇒ boot 层连 `cargo check` 都跑不起来**（实测，见 `reports/`）。
//! ⇒ 本文件**只能由 CI 的 `boot-image` job（QEMU 真实引导）验证**；
//! 其**可镜像的部分**（步长／格式／字节序／回读比对）已在 host 侧断言。
//! **未在本机跑过**，不得声称"已验证"。

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use meta_kernel_core_nostd::fb::{self, FbDesc, FbFormat, Rgb24};
use meta_kernel_core_nostd::project::{self, ProjectSpec};

/// ⑫ 段自检编号的**基准**（`main.rs` 经 `100 + n` 映射后，CI 上读作 **221–225**）。
pub const CODE_ROUNDTRIP_NOT_WRITTEN: u8 = 121;
/// 写入的像素**回读不一致**（编码/字节序/偏移任一环节错）。
pub const CODE_ROUNDTRIP_MISMATCH: u8 = 122;
/// 投影出口给了**非法结果**（规格非法或输入不足 ⇒ 空 `Vec`）。
pub const CODE_PROJECT_INVALID: u8 = 123;
/// 像素格式**不受支持**（`bpp` 越界／`Packed` 起始位 > 24）。
pub const CODE_FORMAT_UNSUPPORTED: u8 = 124;
/// `FbDesc` 与真实缓冲**自相矛盾**（`min_len` 超出 `buf.len()`）。
pub const CODE_DESC_INCONSISTENT: u8 = 125;

/// **`FrameBufferInfo` → `FbDesc`**（本层**唯一**的桥接点）。
///
/// 返回 `None` ＝ 该帧缓冲**不可用**（`bpp` 越界／`stride` 为 0／宽高为 0）——
/// **绝不静默取整或回退**（静默回退会把错位藏起来，判据就失去意义）。
///
/// ⚠️ **单位**：`stride` 在 `FrameBufferInfo` 里是**像素**，与 [`FbDesc::stride_px`] 一致
/// （`verify.rs` 曾在此歧义上险过 ⇒ 本函数把它钉死）。
#[must_use]
pub fn desc_from_info(info: &FrameBufferInfo) -> Option<FbDesc> {
    if info.width == 0 || info.height == 0 || info.stride == 0 {
        return None;
    }
    if info.bytes_per_pixel == 0 || info.bytes_per_pixel > 4 {
        return None;
    }
    let format = match info.pixel_format {
        PixelFormat::Rgb => FbFormat::Rgb,
        PixelFormat::Bgr => FbFormat::Bgr,
        PixelFormat::U8 => FbFormat::U8,
        // `PixelFormat` 是 `#[non_exhaustive]` ⇒ 兜底分支必须存在（与 `verify.rs::write_pixel` 同口径）
        other => match other {
            PixelFormat::Unknown {
                red_position,
                green_position,
                blue_position,
            } => FbFormat::Packed {
                red: red_position,
                green: green_position,
                blue: blue_position,
            },
            // 未来若上游新增变体：**按"不支持"处理**（`encode` 会返回长度 0 ⇒ 调用方判失败）
            _ => FbFormat::Packed { red: 255, green: 255, blue: 255 },
        },
    };
    Some(FbDesc::new(
        info.width as usize,
        info.height as usize,
        info.stride as usize,
        info.bytes_per_pixel as usize,
        format,
    ))
}

/// `FbDesc` 声明的**最小缓冲长度**。
///
/// ⚠️ **不在这里重实现**（C19 同源）：直接转调 [`FbDesc::min_len`] —— 同一台机器事实
/// **只许一份口径实现**，本层只做 `Option` 的用法包装。
#[must_use]
pub fn desc_min_len(d: &FbDesc) -> Option<usize> {
    d.min_len()
}

/// **把场投影并写入真实帧缓冲**（`Project(S)` 的落地点）。返回**实际写入的像素数**。
///
/// 语义：
/// 1. `field` ＋ `spec` → **纯算层**算出灰度序列（[`project::project_gray_u8`]）；
/// 2. 逐像素经 [`fb::put_pixel`] 写进 `buf`（**越界一律 `false`，不 panic**）。
///
/// 返回 `0` ＝ 什么都没写（规格非法／输入不足／格式不支持／缓冲不足）。
/// ⇒ **调用方不得把 `0` 当成功**（C5：不冒充成功）。
#[must_use]
pub fn present_field(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    field: &[f32],
    spec: &ProjectSpec,
) -> usize {
    let Some(d) = desc_from_info(info) else {
        return 0;
    };
    // 缓冲必须装得下 `FbDesc` 声明的全部行（否则后面的"真实硬件写入"会半途截断）
    match desc_min_len(&d) {
        Some(n) if n <= buf.len() => {}
        _ => return 0,
    }
    let gray = project::project_gray_u8(field, spec);
    let Some(cells) = spec.cells() else {
        return 0;
    };
    if gray.is_empty() || gray.len() < cells {
        return 0;
    }
    let mut written = 0usize;
    for (i, &g) in gray.iter().enumerate().take(cells) {
        let x = i % spec.w;
        let y = i / spec.w;
        if fb::put_pixel(buf, &d, x, y, Rgb24::new(g, g, g)) {
            written += 1;
        }
    }
    written
}

/// **⑫ 段自检**：在**真实帧缓冲**上做一次"写入 → 回读 → 逐字节比对"的往返。
///
/// 为什么必须回读（而不是只看返回值）：**返回值只能证明"调用没被拒绝"**，
/// 证明不了"字节真的落在那块内存上"——正是 R14「本机绿是假绿」的同族问题在**硬件侧**的形态。
///
/// 判据（逐条）：
/// - 图案＝2×2，灰度取 `0 / 85 / 170 / 255`（**四值全不同** ⇒ 任何字节序错、行距错、偏移错都会暴露）；
/// - 写入数须 == 4，否则 [`CODE_ROUNDTRIP_NOT_WRITTEN`]；
/// - 回读须与 [`fb::encode`] 的**期望字节逐字节相等**，否则 [`CODE_ROUNDTRIP_MISMATCH`]。
///
/// ⚠️ **退化路径（显式）**：帧缓冲小于 2×2、`FbDesc` 不可用、或格式不支持 ⇒ 返回 `0`
/// （**不判红**）＋ 由调用方在报告里如实标注"本轮未真正验到"。**不假装验过。**
///
/// ⚠️ **回读的适用边界（诚实写清）**：QEMU 的帧缓冲是**普通内存**，回读可靠；
/// **真实硬件上 WC（write-combining）显存回读可能拿到陈旧值** ⇒ 本判据的**适用范围仅 QEMU**。
#[must_use]
pub fn present_selfcheck(buf: &mut [u8], info: &FrameBufferInfo) -> u8 {
    let Some(d) = desc_from_info(info) else {
        return 0; // 退化：帧缓冲不可用（由调用方如实标注）
    };
    if info.width < 2 || info.height < 2 {
        return 0; // 退化：太小，放不下 2×2 图案
    }
    match desc_min_len(&d) {
        Some(n) if n <= buf.len() => {}
        _ => return CODE_DESC_INCONSISTENT,
    }

    // 2×2 图案：四值互异（0, 1/3, 2/3, 1 ⇒ 0, 85, 170, 255）
    let spec = match ProjectSpec::new(2, 2, 0.0, 1.0) {
        Some(s) => s,
        None => return CODE_PROJECT_INVALID,
    };
    let field = [0.0f32, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let gray = project::project_gray_u8(&field, &spec);
    if gray.len() != 4 {
        return CODE_PROJECT_INVALID;
    }

    // 期望字节（**从 `fb::encode` 推导，不手写**，D40／C19）
    let mut written = 0usize;
    for i in 0..4usize {
        let x = i % 2;
        let y = i / 2;
        let g = gray[i];
        if fb::put_pixel(buf, &d, x, y, Rgb24::new(g, g, g)) {
            written += 1;
        } else if fb::encode(&d, Rgb24::new(g, g, g)).1 == 0 {
            return CODE_FORMAT_UNSUPPORTED;
        }
    }
    if written != 4 {
        return CODE_ROUNDTRIP_NOT_WRITTEN;
    }

    // —— 回读：逐像素与 `encode` 的期望字节逐字节比对 ——
    for i in 0..4usize {
        let x = i % 2;
        let y = i / 2;
        let g = gray[i];
        let (bytes, n) = fb::encode(&d, Rgb24::new(g, g, g));
        if n == 0 {
            return CODE_FORMAT_UNSUPPORTED;
        }
        let Some(off) = d.offset_of(x, y) else {
            return CODE_ROUNDTRIP_MISMATCH;
        };
        let Some(end) = off.checked_add(n) else {
            return CODE_ROUNDTRIP_MISMATCH;
        };
        if end > buf.len() {
            return CODE_ROUNDTRIP_MISMATCH;
        }
        // 逐字节（不用切片相等，便于失败时仍能给出确定编号）
        let mut k = 0usize;
        while k < n {
            if buf[off + k] != bytes[k] {
                return CODE_ROUNDTRIP_MISMATCH;
            }
            k += 1;
        }
    }

    0
}
