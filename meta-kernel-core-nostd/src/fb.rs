//! # 帧缓冲抽象层 · **纯算层**（阶段二 **2.4** 前置实现，2026-09-19 夜间落地）
//!
//! 来源：`coordination/discussions/2026-09-19_帧缓冲抽象层设计稿.md` §2.4（关键接口）
//! 与 §2.3.4（落点建议）。**本文件只实现"纯算层"那一半**。
//!
//! ## 为什么在 `nostd` 而不是 `boot`（落点判据，非感觉）
//!
//! 三条铁律推出落点（设计稿 §2.3.1／§2.8）：
//!
//! | 铁律 | 推出什么 |
//! |---|---|
//! | **C1**（内核零依赖） | `bootloader_api` **只许在 boot 层出现** ⇒ 纯算层**必须自定义** [`FbDesc`]/[`FbFormat`]，**不得**复用 `bootloader_api::info::FrameBufferInfo` |
//! | **机制 21**（执行体边界） | "**把数据算成像素字节**" ＝ 纯算；"**持有并改写硬件内存**" ＝ 边界。本层**只接受调用方给的 `&mut [u8]`** ⇒ 拿到缓冲之前的一切都在这层 |
//! | **C9**（`unsafe` 边界） | 帧缓冲**写像素不必 `unsafe`**（拿描述／拿切片／算偏移／写切片**全部安全**）⇒ **本期 0 处新增 `unsafe`**，不需要 C9 放行 |
//!
//! ⇒ 本层可在 host **100% 单测，不需要 QEMU**（设计稿 §2.3.4）。
//!
//! ## 与 "2.2 绿屏" 的关系
//!
//! `meta-kernel-boot/kernel/src/verify.rs` 的 `write_pixel()`（约行 899–937）
//! **已经内含**本层 [`encode`] 的雏形（它把"编码"与"写入"写在一起）。
//! ⇒ 本模块是**把那一半拆出来**（**重构而非重写**）；boot 层若改用它，
//! 只是把"自己算偏移"换成"调 `put_pixel`"，**行为不变**。
//!
//! ## 本层**不做**什么（显式排除，防越权）
//!
//! - ❌ **不碰硬件**：不拿 `bootloader_api::FrameBuffer`、不做 MMIO、不做 `mmap`。
//! - ❌ **不做 DRM/KMS**、不做 GPU 调度、不做显存管理（2.4 主线的定义＝"轻量帧缓冲 + L6 接入"）。
//! - ❌ **不做双缓冲实现**：只留 [`Surface`] 接口（单缓冲 ⇒ `present()` 是 no-op）。
//!   理由：当前分配器**单帧上限 4 KiB**，而 1280×720×3 ＝ 2.64 MiB（差约 675 倍）⇒ 给不出来。
//! - ❌ **不 `panic`**：越界一律返回 `false`/`0`/`None`（裸机上 panic 代价高）。
//!
//! ## 两条"必须显式写清"的单位/口径（本项目已踩过或差点踩到）
//!
//! 1. **`stride_px` 单位是"像素"**（不是字节）——`verify.rs` 曾在此歧义上险过
//!    ⇒ 字段名**显式带 `_px`**。字节步长 ＝ `stride_px * bpp`。
//! 2. **`min_len` 用 `checked_mul`**：`verify.rs` 现用 `saturating_mul`，
//!    二者**语义不同**（`saturating` 把溢出压成 `usize::MAX`，**反而可能放过越界**）
//!    ⇒ 本层统一 `checked_mul`，溢出即 `None`（设计稿 §2.4.1 要点 3，已裁定 Q3）。
//!
//! ## 判据（本文件内的 host 单测，逐条对应裁定）
//!
//! | 判据 | 测什么 | 为什么必须有 |
//! |---|---|---|
//! | **stride 歧义判据** | 刷满屏后再点**一个**像素 ⇒ **只有 `offset 0` 那 `bpp` 个字节变** | 刷满屏时 stride 写错**不会被发现、仍然全绿**；只有单像素才暴露行距错误 |
//! | **裁剪反向断言** | 完全在界外的矩形 ⇒ 返回 `0` **且缓冲逐字节不变** | 只断言"返回 0"证明不了"没写坏内存" |

use crate::math::clamp01;

/// 24 位真彩色（**与格式无关**的语义输入；怎么落字节由 [`encode`] 决定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgb24 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb24 {
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 常用常量（2.2 绿屏用的就是 [`Rgb24::GREEN`]）。
    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);
    pub const RED: Self = Self::new(255, 0, 0);
    pub const GREEN: Self = Self::new(0, 255, 0);
    pub const BLUE: Self = Self::new(0, 0, 255);
}

/// 本层自定义的像素格式（**不依赖 `bootloader_api`**，见文件头 C1 条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbFormat {
    /// 3 字节：`R,G,B`
    Rgb,
    /// 3 字节：`B,G,R`
    Bgr,
    /// 1 字节：灰度（本层用 **Rec.601 亮度** 近似：`0.299R + 0.587G + 0.114B`）
    U8,
    /// 位域格式：各分量在 32 位字里的**起始位**。
    /// 对应 `bootloader_api` 的 `PixelFormat::Unknown`。
    ///
    /// ⚠️ **本层语义（写死，勿猜）**：**每个分量固定 8 位**，起始位由字段给出
    /// ⇒ 可直接表达 `XRGB8888`（`red=16, green=8, blue=0`）这类布局。
    /// **不支持 `RGB565` 那类"窄位域"**（宽 5/6/5）—— 那需要**显式新增一个格式变体**
    /// 并在 [`encode`] 里做**窄化 + 抖动**决策（属"改变像素值"，须先裁定）；
    /// **本层拒绝隐式窄化**（否则会静默改变颜色，且判据难以覆盖）。
    /// `起始位 > 24`（8 位放不下）⇒ 一律按**不支持**处理（[`encode`] 返回长度 0）。
    Packed { red: u8, green: u8, blue: u8 },
}

/// 帧缓冲描述（**纯数据**；不含任何指针/硬件句柄）。
///
/// ⚠️ **字段顺序刻意与 `bootloader_api::info::FrameBufferInfo` 不同**：本层**不依赖**它，
/// 只做字段级对应（`byte_len`/`buffer` 由**边界层**持有，**不进本结构**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FbDesc {
    /// 可视宽度（**像素**）
    pub width: usize,
    /// 可视高度（**像素**）
    pub height: usize,
    /// 行距（**像素**，不是字节！）
    pub stride_px: usize,
    /// 每像素字节数（`bytes_per_pixel`，1..=4）
    pub bpp: usize,
    /// 像素格式
    pub format: FbFormat,
}

impl FbDesc {
    #[must_use]
    pub const fn new(width: usize, height: usize, stride_px: usize, bpp: usize, format: FbFormat) -> Self {
        Self { width, height, stride_px, bpp, format }
    }

    /// 2.2 绿屏的同款描述（QEMU 1280×720、`Bgr` 24bpp、stride == width）。
    #[must_use]
    pub const fn qemu_1280x720_bgr() -> Self {
        Self::new(1280, 720, 1280, 3, FbFormat::Bgr)
    }

    /// 缓冲区所需**最小字节数**；**溢出 ⇒ `None`**（**不是** `saturating`，见文件头口径 2）。
    #[must_use]
    pub fn min_len(&self) -> Option<usize> {
        self.stride_px.checked_mul(self.height)?.checked_mul(self.bpp)
    }

    /// `(x, y)` → **字节偏移**。越界／溢出 ⇒ `None`（**不 panic、不 wrap**）。
    ///
    /// 注意：判定用 **`width`**（可视宽），而**步进用 `stride_px`** —— 这正是"stride 可 > width"
    /// 的语义（GOP 式按行对齐），也是本层第一条判据要钉的地方。
    #[must_use]
    pub fn offset_of(&self, x: usize, y: usize) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        y.checked_mul(self.stride_px)?.checked_add(x)?.checked_mul(self.bpp)
    }

    /// 该描述是否自洽（`bpp` 与格式匹配、`stride_px ≥ width`）。
    #[must_use]
    pub fn is_sane(&self) -> bool {
        if self.bpp == 0 || self.bpp > 4 || self.stride_px < self.width {
            return false;
        }
        match self.format {
            FbFormat::Rgb | FbFormat::Bgr => self.bpp == 3,
            FbFormat::U8 => self.bpp == 1,
            FbFormat::Packed { red, green, blue } => {
                self.bpp <= 4 && red <= 24 && green <= 24 && blue <= 24
            }
        }
    }

    /// 给定缓冲是否够大（够用 ⇒ `Ok(所需字节)`，否则 `Err(所需字节)`）。
    pub fn check_buf(&self, buf_len: usize) -> Result<usize, usize> {
        match self.min_len() {
            Some(need) if need <= buf_len => Ok(need),
            Some(need) => Err(need),
            None => Err(usize::MAX), // 溢出：需求上界都算不出来 ⇒ 一律判不足
        }
    }
}

/// 半开区间矩形 `[x0, x1) × [y0, y1)`（**半开**是刻意选择：`x1 - x0` 直接等于宽度，免去 `+1` 类差一错误）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl Rect {
    #[must_use]
    pub const fn new(x0: usize, y0: usize, x1: usize, y1: usize) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// 整幅画面。
    #[must_use]
    pub const fn full(w: usize, h: usize) -> Self {
        Self::new(0, 0, w, h)
    }

    #[must_use]
    pub const fn width(&self) -> usize {
        self.x1.saturating_sub(self.x0)
    }

    #[must_use]
    pub const fn height(&self) -> usize {
        self.y1.saturating_sub(self.y0)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// 与 `[0, w) × [0, h)` 求交（**裁剪**；空交集 ⇒ 宽度/高度为 0）。
    #[must_use]
    pub fn clipped(&self, w: usize, h: usize) -> Self {
        let x0 = if self.x0 > w { w } else { self.x0 };
        let y0 = if self.y0 > h { h } else { self.y0 };
        let x1 = if self.x1 > w { w } else { self.x1 };
        let y1 = if self.y1 > h { h } else { self.y1 };
        if x1 <= x0 || y1 <= y0 {
            // 显式归零，保证 `is_empty()` 恒真（避免 `x0 > x1` 这类"负宽度"表示）
            Self::new(x0, y0, x0, y0)
        } else {
            Self::new(x0, y0, x1, y1)
        }
    }
}

/// **编码函数（纯算的关键出口）**：`Rgb24` → 该格式的字节序列 ＋ 有效长度。
///
/// **返回长度 `0` 表示"该格式/参数不受支持"**（调用方须据此判失败）——
/// 这是本层**唯一**的失败编码约定（不用 `Option` 以免在中断上下文里产生分支预测代价，
/// 也便于 `const` 使用）。
///
/// 与现状对应：**等价于** `verify.rs::write_pixel()` **去掉"写入"那一半**。
#[must_use]
pub fn encode(d: &FbDesc, rgb: Rgb24) -> ([u8; 4], usize) {
    let Rgb24 { r, g, b } = rgb;
    match d.format {
        FbFormat::Rgb => ([r, g, b, 0], 3),
        FbFormat::Bgr => ([b, g, r, 0], 3),
        FbFormat::U8 => {
            // Rec.601 亮度（整数加权，避免引入浮点；权重和 = 1000）
            let y = (299u32 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000;
            ([y as u8, 0, 0, 0], 1)
        }
        FbFormat::Packed { red, green, blue } => {
            if red > 24 || green > 24 || blue > 24 || d.bpp == 0 || d.bpp > 4 {
                return ([0; 4], 0);
            }
            let v = (u32::from(r) << red) | (u32::from(g) << green) | (u32::from(b) << blue);
            let le = v.to_le_bytes();
            let mut out = [0u8; 4];
            let n = d.bpp;
            let mut i = 0;
            while i < n {
                out[i] = le[i];
                i += 1;
            }
            (out, n)
        }
    }
}

/// **单像素写**：把 `rgb` 按 `d.format` 编码后写入 `buf`。
///
/// 返回 `false` ＝ 越界／格式不支持／缓冲不足（**不 panic**、**不写任何字节**）。
pub fn put_pixel(buf: &mut [u8], d: &FbDesc, x: usize, y: usize, rgb: Rgb24) -> bool {
    let (bytes, n) = encode(d, rgb);
    if n == 0 {
        return false;
    }
    let off = match d.offset_of(x, y) {
        Some(o) => o,
        None => return false,
    };
    let end = match off.checked_add(n) {
        Some(e) => e,
        None => return false,
    };
    if end > buf.len() {
        return false;
    }
    buf[off..end].copy_from_slice(&bytes[..n]);
    true
}

/// **矩形填充**（半开区间，**先裁剪再画**）。
///
/// 返回**实际写入的像素数** ⇒ 可直接断言"裁剪是否生效"（界外 ⇒ `0`）。
pub fn fill_rect(buf: &mut [u8], d: &FbDesc, r: Rect, rgb: Rgb24) -> usize {
    let (bytes, n) = encode(d, rgb);
    if n == 0 {
        return 0;
    }
    let c = r.clipped(d.width, d.height);
    if c.is_empty() {
        return 0;
    }
    let row_bytes = match d.stride_px.checked_mul(d.bpp) {
        Some(v) => v,
        None => return 0,
    };
    let mut count = 0usize;
    let mut y = c.y0;
    while y < c.y1 {
        let row_off = match y.checked_mul(row_bytes) {
            Some(v) => v,
            None => return count,
        };
        let mut x = c.x0;
        while x < c.x1 {
            let off = match x.checked_mul(d.bpp).and_then(|c2| row_off.checked_add(c2)) {
                Some(v) => v,
                None => return count,
            };
            let end = match off.checked_add(n) {
                Some(v) => v,
                None => return count,
            };
            if end > buf.len() {
                // 缓冲不足：**停止并如实返回已写量**（不 panic、不越界）
                return count;
            }
            buf[off..end].copy_from_slice(&bytes[..n]);
            count += 1;
            x += 1;
        }
        y += 1;
    }
    count
}

/// **位块复制**（同一缓冲内的区域搬运；**处理行内重叠**）。
///
/// 本期为**可选**接口（设计稿 §2.4.2）：只在 L6 确需"把一块现有画面搬走"时才用。
/// 返回实际搬运的像素数。**空交集 / 缓冲不足 ⇒ `0`，且不写任何字节。**
pub fn blit(buf: &mut [u8], d: &FbDesc, src: Rect, dst: (usize, usize)) -> usize {
    if d.bpp == 0 || d.bpp > 4 {
        return 0;
    }
    let s = src.clipped(d.width, d.height);
    if s.is_empty() {
        return 0;
    }
    // 目标锚点 (dx, dy) 可越界 ⇒ 逐像素判
    let row_bytes = match d.stride_px.checked_mul(d.bpp) {
        Some(v) => v,
        None => return 0,
    };
    let n = d.bpp;
    let w = s.width();
    let h = s.height();
    // 若目标在源的下方 ⇒ 自下而上搬，避免覆盖源数据（行内重叠由 copy_within 处理）
    let bottom_up = dst.1 >= s.y0;
    let mut i = 0usize;
    let mut count = 0usize;
    while i < h {
        let src_y = if bottom_up { s.y0 + (h - 1 - i) } else { s.y0 + i };
        let dst_y = if bottom_up { dst.1 + (h - 1 - i) } else { dst.1 + i };
        if dst_y < d.height && src_y < d.height {
            let so = match src_y.checked_mul(row_bytes).and_then(|v| v.checked_add(s.x0 * n)) {
                Some(v) => v,
                None => return count,
            };
            let doff = match dst_y.checked_mul(row_bytes).and_then(|v| v.checked_add(dst.0 * n)) {
                Some(v) => v,
                None => return count,
            };
            let len = match w.checked_mul(n) {
                Some(v) => v,
                None => return count,
            };
            if so + len <= buf.len() && doff + len <= buf.len() {
                // `copy_within` 是 memmove 语义 ⇒ 行内重叠安全
                buf.copy_within(so..so + len, doff);
                count += w;
            }
        }
        i += 1;
    }
    count
}

// ===================== 上采样（"粗网格 → 全分辨率"的**算法**部分） =====================
//
// 口径（设计稿 §2.2.4 末条）：**"上采样数学"是纯算**（可写可测）；
// **"上采样到哪里"（那块硬件内存）是边界**。⇒ 本层只输出到调用方的 `out` 切片。

/// 最近邻上采样（`RGB24`，3 字节/像素）。
///
/// 返回写入的**像素数**（超出 `out` 的部分被跳过，不 panic）。
pub fn upscale_nearest_rgb24(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    out: &mut [u8],
) -> usize {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return 0;
    }
    if src.len() < src_w.saturating_mul(src_h).saturating_mul(3) {
        return 0;
    }
    let mut n = 0usize;
    let mut y = 0usize;
    while y < dst_h {
        // 整数映射：`sy = y * src_h / dst_h`（**不引入浮点** ⇒ 结果可复现）
        let sy = y * src_h / dst_h;
        let mut x = 0usize;
        while x < dst_w {
            let sx = x * src_w / dst_w;
            let so = (sy * src_w + sx) * 3;
            let doff = (y * dst_w + x) * 3;
            if doff + 3 > out.len() {
                return n;
            }
            out[doff..doff + 3].copy_from_slice(&src[so..so + 3]);
            n += 1;
            x += 1;
        }
        y += 1;
    }
    n
}

/// 双线性上采样（`RGB24`）。**用 16 位定点权重**（`u32` 运算）⇒ **确定性**、无 `sin/cos` 依赖。
pub fn upscale_bilinear_rgb24(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    out: &mut [u8],
) -> usize {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return 0;
    }
    if src.len() < src_w.saturating_mul(src_h).saturating_mul(3) {
        return 0;
    }
    const F: u64 = 1 << 16; // 定点分母
    let (sw, sh, dw, dh) = (src_w as u64, src_h as u64, dst_w as u64, dst_h as u64);
    let half = F / 2;
    let mut n = 0usize;
    let mut y = 0usize;
    while y < dst_h {
        // 采样点落在像素**中心**：`v = (y + 0.5) * src_h / dst_h - 0.5`
        // 定点化：(2y+1)·src_h·F / (2·dst_h) − F/2
        let fy = ((2 * (y as u64) + 1) * sh * F) / (2 * dh);
        let fy = fy.saturating_sub(half);
        let y0 = (fy / F) as usize;
        let wy = (fy % F) as u32;
        let y1 = if y0 + 1 < src_h { y0 + 1 } else { y0 };
        let mut x = 0usize;
        while x < dst_w {
            let fx = ((2 * (x as u64) + 1) * sw * F) / (2 * dw);
            let fx = fx.saturating_sub(half);
            let x0 = (fx / F) as usize;
            let wx = (fx % F) as u32;
            let x1 = if x0 + 1 < src_w { x0 + 1 } else { x0 };

            let c00 = (y0 * src_w + x0) * 3;
            let c10 = (y0 * src_w + x1) * 3;
            let c01 = (y1 * src_w + x0) * 3;
            let c11 = (y1 * src_w + x1) * 3;
            let doff = (y * dst_w + x) * 3;
            if doff + 3 > out.len() {
                return n;
            }
            for k in 0..3usize {
                let a = u64::from(src[c00 + k]);
                let b = u64::from(src[c10 + k]);
                let c = u64::from(src[c01 + k]);
                let e = u64::from(src[c11 + k]);
                // 双线性：先水平后垂直（全整数，权重 F）
                let top = a * (F - u64::from(wx)) + b * u64::from(wx);
                let bot = c * (F - u64::from(wx)) + e * u64::from(wx);
                let v = top * (F - u64::from(wy)) + bot * u64::from(wy);
                // 四舍五入到整数
                let v = (v + (F * F) / 2) / (F * F);
                let v = if v > 255 { 255 } else { v };
                out[doff + k] = v as u8;
            }
            n += 1;
            x += 1;
        }
        y += 1;
    }
    n
}

/// 把 `f32 ∈ [0,1]` 的**归一化亮度**量化为 `u8`（**投影函数的唯一"格式"出口**）。
///
/// 用 [`clamp01`] 保证"呈现不增义"：**只做截断，不做任何风格化/非线性变换**。
#[must_use]
pub fn quantize_u8(v: f32) -> u8 {
    let t = clamp01(v);
    (t * 255.0 + 0.5) as u8
}

// ===================== 呈现目标抽象（留接口，本期只有单缓冲） =====================

/// 呈现目标抽象。
///
/// **为什么不现在做双缓冲**（设计稿 §2.4.4）：本期画面是"自检信号 + 静态呈现"，**没有动画**；
/// 且当前分配器**单帧上限 4 KiB**，而 1280×720×3 ＝ 2,764,800 B ≈ 2.64 MiB（差约 **675 倍**）。
/// ⇒ **只留 trait**：将来多缓冲 ＝ "新增一个 impl ＋ 分配器加 `alloc_run(n)` 原语"。
pub trait Surface {
    fn desc(&self) -> &FbDesc;
    fn pixels(&mut self) -> &mut [u8];
    /// 单缓冲 ⇒ no-op；将来多缓冲 ⇒ 在此 swap。
    fn present(&mut self) {}
}

/// 单缓冲直写（本期唯一实现）。`buf` **由边界层提供**（本层不持有硬件内存）。
#[derive(Debug)]
pub struct SingleFb<'a> {
    buf: &'a mut [u8],
    desc: FbDesc,
}

impl<'a> SingleFb<'a> {
    /// 描述与缓冲必须匹配（不匹配 ⇒ `Err(所需字节数)`）。
    pub fn new(buf: &'a mut [u8], desc: FbDesc) -> Result<Self, usize> {
        desc.check_buf(buf.len())?;
        Ok(Self { buf, desc })
    }

    /// 便捷入口：直接往自己这块画（等价于 `fill_rect(pixels(), ...)`）。
    pub fn fill(&mut self, r: Rect, rgb: Rgb24) -> usize {
        fill_rect(self.buf, &self.desc, r, rgb)
    }
}

impl Surface for SingleFb<'_> {
    fn desc(&self) -> &FbDesc {
        &self.desc
    }

    fn pixels(&mut self) -> &mut [u8] {
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(w: usize, h: usize) -> FbDesc {
        FbDesc::new(w, h, w, 3, FbFormat::Bgr)
    }

    // ---------- ① 描述结构：checked_mul 与越界 ----------

    /// **Q3 裁定落点**：`min_len` 溢出必须 `None`（**不得** `saturating` 成 `usize::MAX`）。
    ///
    /// **为什么必须有**：`saturating_mul` 会把溢出压成极大值，若调用方写成 `need <= buf.len()`
    /// 就**恒假**（安全但错报），若写成 `need == buf.len()` 之类则可能**放过越界**。
    /// 本测钉住"溢出 ⇒ None"这一语义。
    #[test]
    fn min_len_uses_checked_mul_not_saturating() {
        // stride*height 溢出
        let d = FbDesc::new(1, usize::MAX, usize::MAX, 3, FbFormat::Bgr);
        assert_eq!(d.min_len(), None, "溢出必须 None（saturating 会给出 usize::MAX ⇒ 放过越界）");
        // 正常值
        let d2 = FbDesc::new(1280, 720, 1280, 3, FbFormat::Bgr);
        assert_eq!(d2.min_len(), Some(1280 * 720 * 3));
        assert_eq!(d2.min_len(), Some(2_764_800), "与帧缓冲稿 §2.4.4 的 2.64 MiB 对照");
        // bpp 相乘那一步也溢出
        let d3 = FbDesc::new(1, 1, usize::MAX / 2, 4, FbFormat::Packed { red: 16, green: 8, blue: 0 });
        assert_eq!(d3.min_len(), None);
        assert_eq!(d3.check_buf(usize::MAX), Err(usize::MAX), "溢出 ⇒ 一律判缓冲不足");
    }

    #[test]
    fn offset_of_bounds_and_stride_semantics() {
        // stride > width：按行对齐的 GOP 式布局
        let d = FbDesc::new(3, 2, 5, 3, FbFormat::Bgr);
        assert_eq!(d.offset_of(0, 0), Some(0));
        assert_eq!(d.offset_of(2, 0), Some(6));
        assert_eq!(d.offset_of(0, 1), Some(15), "第 2 行起点 = stride_px*bpp = 5*3");
        assert_eq!(d.offset_of(3, 0), None, "x == width ⇒ 越界（可视区外，即便 stride 有富余）");
        assert_eq!(d.offset_of(0, 2), None);
        assert_eq!(d.offset_of(usize::MAX, 0), None);
        assert_eq!(d.offset_of(0, usize::MAX), None, "乘算溢出 ⇒ None");
        assert!(d.is_sane());
        assert!(!FbDesc::new(10, 1, 2, 3, FbFormat::Rgb).is_sane(), "stride < width ⇒ 不自洽");
        assert!(!FbDesc::new(1, 1, 1, 2, FbFormat::Rgb).is_sane(), "bpp 与格式不匹配 ⇒ 不自洽");
    }

    // ---------- ② 编码（纯算最易测的一半） ----------

    #[test]
    fn encode_all_formats() {
        let rgb = Rgb24::new(1, 2, 3);
        let (b, n) = encode(&FbDesc::new(1, 1, 1, 3, FbFormat::Rgb), rgb);
        assert_eq!((&b[..n], n), (&[1u8, 2, 3][..], 3));
        let (b, n) = encode(&FbDesc::new(1, 1, 1, 3, FbFormat::Bgr), rgb);
        assert_eq!((&b[..n], n), (&[3u8, 2, 1][..], 3), "Bgr 是字节序反转");
        // 灰度：Rec.601 整数加权
        let (b, n) = encode(&FbDesc::new(1, 1, 1, 1, FbFormat::U8), Rgb24::GREEN);
        assert_eq!((b[0], n), (149u8, 1), "0.587*255 = 149.7 ⇒ 149");
        let (b, n) = encode(&FbDesc::new(1, 1, 1, 1, FbFormat::U8), Rgb24::WHITE);
        assert_eq!(b[0], 255);
        let (b, n) = encode(&FbDesc::new(1, 1, 1, 1, FbFormat::U8), Rgb24::BLACK);
        assert_eq!(b[0], 0);
        // Packed：XRGB8888 式（各分量 8 位，起始位由字段给出），bpp=4
        let d = FbDesc::new(1, 1, 1, 4, FbFormat::Packed { red: 16, green: 8, blue: 0 });
        let (b, n) = encode(&d, Rgb24::new(0x11, 0x22, 0x33));
        assert_eq!(n, 4);
        assert_eq!(u32::from_le_bytes(b), 0x0011_2233, "R@16 G@8 B@0 ⇒ 0x00112233");
        // 同一位域但 bpp=3 ⇒ 高字节被截断（低 3 字节仍是 B,G,R）
        let d3 = FbDesc::new(1, 1, 1, 3, FbFormat::Packed { red: 16, green: 8, blue: 0 });
        let (b3, n3) = encode(&d3, Rgb24::new(0x11, 0x22, 0x33));
        assert_eq!(n3, 3);
        assert_eq!(&b3[..3], &[0x33u8, 0x22, 0x11]);
        // 不支持的位域（8 位放不下）⇒ 长度 0
        let bad = FbDesc::new(1, 1, 1, 4, FbFormat::Packed { red: 40, green: 8, blue: 0 });
        assert_eq!(encode(&bad, rgb).1, 0, "起始位 > 24 ⇒ 不支持（长度 0）");
        assert!(!bad.is_sane());
        assert!(!put_pixel(&mut [0u8; 4], &bad, 0, 0, rgb), "不支持的格式 ⇒ 不写、返回 false");
    }

    // ---------- ③ ★ stride 歧义判据 ----------

    /// **★ 判据：stride 歧义。**
    ///
    /// **做法**：整屏刷绿（此时**无论 stride 写对写错都是全绿** ⇒ 该判据**测不出错**），
    /// 然后在 `(0,0)` 点**一个**红像素 ⇒ **必须只有字节 `[0, 3)` 变化**。
    ///
    /// **为什么必须有**：`stride` 被误当"字节"（或误当 `width`）时，**刷满屏仍然是绿的**
    /// —— 缺陷**全程不可见**；只有"单像素 + 逐字节比对"才能钉住行距语义。
    /// （本项目在 `verify.rs` 已踩过"stride 是像素还是字节"的歧义，所幸当时写对。）
    #[test]
    fn stride_ambiguity_single_pixel_touches_only_offset_zero() {
        let d = FbDesc::new(4, 3, 4, 3, FbFormat::Bgr); // stride == width ⇒ 无行填充
        let mut buf = vec![0u8; d.min_len().unwrap()];
        let full = fill_rect(&mut buf, &d, Rect::full(4, 3), Rgb24::GREEN);
        assert_eq!(full, 12, "整屏 12 像素全绿");
        let before = buf.clone();
        assert!(put_pixel(&mut buf, &d, 0, 0, Rgb24::RED));
        let mut changed: Vec<usize> = Vec::new();
        for i in 0..buf.len() {
            if buf[i] != before[i] {
                changed.push(i);
            }
        }
        assert_eq!(
            changed,
            vec![1usize, 2],
            "★ 只有 offset 1..3 变 —— 注意 **offset 0 不在里面**：`Bgr` 下绿 = [0,255,0]、红 = [0,0,255]，\n\
             两者的 **B 分量都是 0** ⇒ 第 0 字节**本来就相同**。\n\
             （我初版把期望写成 [0,1,2] ⇒ 判红；核对格式后确认是**期望值写错**。）"
        );
        assert_eq!(&buf[0..3], &[0u8, 0, 255], "Bgr 下红色写在头 3 字节，且是 B,G,R = 0,0,255");

        // 对照：stride > width 时，第 2 行第 1 像素落在 stride*bpp，而不是 width*bpp
        let d2 = FbDesc::new(4, 3, 8, 3, FbFormat::Bgr);
        let mut buf2 = vec![0u8; d2.min_len().unwrap()];
        assert!(put_pixel(&mut buf2, &d2, 0, 1, Rgb24::BLUE));
        let first_changed = buf2.iter().position(|&v| v != 0).unwrap();
        assert_eq!(first_changed, 24, "★ 行距按 stride_px(8) 而非 width(4)：8*3 = 24");
    }

    // ---------- ④ ★ 裁剪反向断言 ----------

    /// **★ 判据：裁剪反向断言。**
    ///
    /// 完全在界外的矩形 ⇒ **返回 `0`** **且缓冲逐字节不变**。
    ///
    /// **为什么必须是"逐字节"**：只断言"返回 0"证明不了"没写坏内存"
    /// （完全可能"返回 0 但仍写了几个字节"——那正是最危险的情形：静默越界写）。
    #[test]
    fn fill_rect_out_of_bounds_writes_nothing_byte_for_byte() {
        let d = FbDesc::new(4, 3, 4, 3, FbFormat::Bgr);
        let mut buf = vec![0xABu8; d.min_len().unwrap()];
        let snapshot = buf.clone();

        // 四种"全在界外"的形态
        let cases = [
            Rect::new(10, 10, 12, 12),   // 右下方界外
            Rect::new(0, 5, 4, 8),       // 下方
            Rect::new(4, 0, 8, 3),       // 右侧（x0 == width 起）
            Rect::new(2, 2, 2, 2),       // 空矩形
        ];
        for (i, r) in cases.iter().enumerate() {
            let n = fill_rect(&mut buf, &d, *r, Rgb24::RED);
            assert_eq!(n, 0, "case {i}: 界外/空矩形必须返回 0");
            assert_eq!(buf, snapshot, "case {i}: ★ 必须逐字节不变（只断返回 0 是不够的）");
        }
        // put_pixel 同款反向断言
        assert!(!put_pixel(&mut buf, &d, 4, 0, Rgb24::RED));
        assert!(!put_pixel(&mut buf, &d, 0, 3, Rgb24::RED));
        assert_eq!(buf, snapshot, "put_pixel 越界也必须逐字节不变");
    }

    /// 部分裁剪：返回**实际写入像素数** ＝ 交集的面积（可手算）。
    #[test]
    fn fill_rect_clips_and_counts_exactly() {
        let d = FbDesc::new(5, 4, 5, 3, FbFormat::Bgr);
        let mut buf = vec![0u8; d.min_len().unwrap()];
        // 请求 [3,8) × [2,6) ⇒ 与 [0,5)×[0,4) 求交 = [3,5) × [2,4) = 2×2 = 4
        let n = fill_rect(&mut buf, &d, Rect::new(3, 2, 8, 6), Rgb24::WHITE);
        assert_eq!(n, 4);
        // 逐像素核对：只有那 4 个位置被写
        let mut written = Vec::new();
        for y in 0..4 {
            for x in 0..5 {
                if buf[d.offset_of(x, y).unwrap()] == 255 {
                    written.push((x, y));
                }
            }
        }
        assert_eq!(written, vec![(3, 2), (4, 2), (3, 3), (4, 3)]);
    }

    /// 缓冲不足 ⇒ **不 panic**，如实返回已写量（裸机不能 panic）。
    #[test]
    fn fill_rect_short_buffer_does_not_panic() {
        let d = FbDesc::new(4, 4, 4, 3, FbFormat::Bgr);
        let mut buf = vec![0u8; 3 * 3]; // 只够 3 像素
        let n = fill_rect(&mut buf, &d, Rect::full(4, 4), Rgb24::GREEN);
        assert_eq!(n, 3, "只写了 3 个像素就停（不 panic、不越界）");
        assert!(buf.iter().all(|&v| v == 0 || v == 255), "绿(Bgr)=[0,255,0] ⇒ 字节只能是 0 或 255");
    }

    // ---------- ⑤ blit / 上采样 / Surface ----------

    #[test]
    fn blit_moves_region_and_handles_overlap() {
        let d = FbDesc::new(6, 1, 6, 3, FbFormat::Bgr);
        let mut buf = vec![0u8; d.min_len().unwrap()];
        // 第 0 行：前 3 像素红，后 3 像素绿
        fill_rect(&mut buf, &d, Rect::full(6, 1), Rgb24::GREEN);
        fill_rect(&mut buf, &d, Rect::new(0, 0, 3, 1), Rgb24::RED);
        let is_red = |buf: &[u8], x: usize| {
            let o = d.offset_of(x, 0).unwrap();
            buf[o] == 0 && buf[o + 2] == 255
        };
        // 把 [0,3) 搬到 (2,0) ⇒ 位置 2/3/4 应变红、5 仍绿 ⇒ R R R R R G
        let n = blit(&mut buf, &d, Rect::new(0, 0, 3, 1), (2, 0));
        assert_eq!(n, 3);
        let expect = [
            (0usize, true),
            (1, true),
            (2, true),
            (3, true),
            (4, true),
            (5, false), // ★ 这一格是判据：搬运**不多写一格**（长度算错就会变红）
        ];
        for (x, want_red) in expect {
            assert_eq!(is_red(&buf, x), want_red, "x={x} 期望红={want_red}");
        }
        // 空源 ⇒ 0 且不变
        let snap = buf.clone();
        assert_eq!(blit(&mut buf, &d, Rect::new(9, 9, 12, 12), (0, 0)), 0);
        assert_eq!(buf, snap);
    }

    #[test]
    fn upscale_nearest_and_bilinear_bounds() {
        // 2×2 输入 → 4×4 输出
        let src: Vec<u8> = vec![
            255, 0, 0, /*(0,0) 红*/ 0, 255, 0, /*(1,0) 绿*/
            0, 0, 255, /*(0,1) 蓝*/ 255, 255, 255, /*(1,1) 白*/
        ];
        let mut out = vec![0u8; 4 * 4 * 3];
        let n = upscale_nearest_rgb24(&src, 2, 2, 4, 4, &mut out);
        assert_eq!(n, 16);
        // 每个 2×2 块同色
        let px = |x: usize, y: usize| {
            let o = (y * 4 + x) * 3;
            (out[o], out[o + 1], out[o + 2])
        };
        assert_eq!(px(0, 0), (255, 0, 0));
        assert_eq!(px(1, 1), (255, 0, 0));
        // 映射式：sy = y*src_h/dst_h = 2*2/4 = 1，sx = x*src_w/dst_w = 2*2/4 = 1 ⇒ src(1,1) = 白
        assert_eq!(px(2, 2), (255, 255, 255));
        assert_eq!(px(3, 3), (255, 255, 255));
        assert_eq!(px(2, 0), (0, 255, 0), "(2,0)→src(1,0) 绿");

        // 双线性：常量图必须**逐像素等于常量**（最强的正向对照）
        let flat: Vec<u8> = vec![7u8; 3 * 3 * 3];
        let mut out2 = vec![0u8; 5 * 5 * 3];
        let n2 = upscale_bilinear_rgb24(&flat, 3, 3, 5, 5, &mut out2);
        assert_eq!(n2, 25);
        assert!(out2.iter().all(|&v| v == 7), "常量图双线性 ⇒ 仍为常量（无波纹/无溢出）");

        // 边界：源/目标为 0 ⇒ 0；out 太小 ⇒ 不 panic
        assert_eq!(upscale_nearest_rgb24(&src, 0, 2, 4, 4, &mut out), 0);
        let mut tiny = vec![0u8; 4];
        let n3 = upscale_nearest_rgb24(&src, 2, 2, 4, 4, &mut tiny);
        assert_eq!(n3, 1, "只放得下 1 个像素（不 panic）");
    }

    #[test]
    fn surface_single_fb_and_quantize() {
        let d = FbDesc::qemu_1280x720_bgr();
        assert_eq!(d.min_len(), Some(2_764_800));
        let mut buf = vec![0u8; d.min_len().unwrap()];
        let mut s = SingleFb::new(&mut buf, d).expect("缓冲足够");
        assert_eq!(s.desc().width, 1280);
        assert_eq!(s.fill(Rect::full(1280, 720), Rgb24::GREEN), 1280 * 720);
        assert_eq!(s.pixels()[0], 0, "Bgr：绿 = B=0");
        assert_eq!(s.pixels()[1], 255);
        assert_eq!(s.pixels()[2], 0);
        s.present(); // 单缓冲 ⇒ no-op（不 panic）
        // 缓冲不足 ⇒ Err(所需字节)
        let mut small = vec![0u8; 10];
        assert_eq!(SingleFb::new(&mut small, d).unwrap_err(), 2_764_800);

        // 量化：0/1/越界 与 "呈现不增义"（只截断，不做非线性）
        assert_eq!(quantize_u8(0.0), 0);
        assert_eq!(quantize_u8(1.0), 255);
        assert_eq!(quantize_u8(-5.0), 0, "下越界被 clamp（不 wrap）");
        assert_eq!(quantize_u8(5.0), 255);
        assert_eq!(quantize_u8(0.5), 128);
    }
}
