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
//! ## 本文件的三条入口（★ 2026-09-20 起：product 路径接线；★ 同日：场演化小范围验证）
//! | 入口 | 编号（裸机→CI） | 走不走 `present_field` | 场源 | 用途 |
//! |---|---|---|---|---|
//! | [`present_selfcheck`] | **121–125** → 221–225 | ❌ 否（写死 2×2 图案） | 无 | 验**桥接与写入**（`FrameBufferInfo` → `FbDesc` → 字节） |
//! | [`present_product_selftest`] | **126–128** → 226–228 | ✅ **是** | **静态**（SDF 一帧） | 验 **product 接线＋回读**（回归基线） |
//! | [`present_evolve_selftest`] | **129–132** → 229–232 | ✅ **是** | **多步演化**（`nca::evolve`） | 验 **"场演化 → 投影 → 像素"链真的通** |
//! 三条都**回读校验**；**缺任一条** ⇒ "桥接正确"／"产品入口可用"／"场演化链通"就分不清（**漏挂 = 静默空转**）。
//!
//! ## ★ 退化出口**机器化**（**R83**；2026-09-20）
//! 旧写法：退化（帧缓冲不可用／可视区过小）**返回 `0`** —— 而 `0` 又是"全过"的值
//! ⇒ **"没验到"与"验到了"同值** ⇒ 判据**静默变绿**。
//! 现写法：退化经纯算层 [`fb::classify_degenerate`]（**判定属纯算 ⇒ 在 `nostd`，本机可测**）分类，
//! 映射为**非零编号** ⇒ `main()` `Fail(100 + r)` ⇒ **判红**（编号 **133／134**）。
//! ⇒ 本层的三条入口**不再有"退化成绿"的路径**（**"未验到"必须显式报红**）。
//!
//! ## ⚠️ 诚实标注（C5：本机可做/不可做，**口径已精确化**）
//! - ✅ **可做**：用 **GNU 宿主工具链**做类型检查 —— ★ **命令口径已订正两次，见 `BASELINE.md` R85**：
//!   **须先** `cd meta-kernel-boot`（boot 是**独立 workspace**；且 `.cargo/config.toml` 的 `bindeps = true`
//!   靠 **cwd 向上查找**生效，是**必需**配置），再执行：
//!   `cargo +nightly-x86_64-pc-windows-gnu check -p meta-kernel-boot-kernel --target x86_64-unknown-none --target-dir "<系统 Temp>/_mk_boot_probe"`
//!   （**实测通过，error 0**）。**为何要 `--target-dir`**：往 **workspace 内**写构建产物在本机**不稳**
//!   （实测 **0/3** 成功），**系统 Temp 3/3 成功** —— 属**环境因**，非本文件缺陷。
//!   另：**必须显式 GNU 工具链** —— `bootloader_api` 的宿主构建脚本在 MSVC 宿主下会缺导入库。
//! - ❌ **不可做**：**full build**（缺 `dlltool.exe`）与 **QEMU 真实引导**（本机无 `qemu-system-x86_64`）
//!   ⇒ 本文件**只能由 CI 的 `boot-image` job（QEMU 真实引导）证成**；
//!   其**可镜像的部分**（步长／格式／字节序／回读比对／退化分类）已在 host 侧断言。
//! - **未在本机跑过真实引导**，不得声称"已验证"。

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use meta_kernel_core_nostd::fb::{self, classify_degenerate, Degenerate, FbDesc, FbFormat, Rgb24};
use meta_kernel_core_nostd::field::{nca, sdf};
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

/// **product 路径**自检编号的**基准**（`main.rs` 经 `100 + n` 映射后，CI 上读作 **226–228**）。
/// （121–125 是 ⑫ 段的**自检路径**编号；126–128 是**产品路径**编号 —— 两段分开，便于定位。）
pub const CODE_PRODUCT_NOT_WRITTEN: u8 = 126;
/// product 路径**回读不一致**（`present_field` 写进去的字节，读回来对不上）。
pub const CODE_PRODUCT_MISMATCH: u8 = 127;
/// product 路径的**场源／投影出口非法**（SDF 采样不足 或 投影为空）。
pub const CODE_PRODUCT_EMPTY: u8 = 128;

// ---------------------------------------------------------------------------
// 场演化路径编号（129–132）→ CI 上读作 **229–232**（同样经 `100 + n` 映射）
// 为什么单列一段：`present_product_selftest` 的场是**静态**的（SDF 一帧），
// 它证不了"**场演化**→投影→像素"这条链 —— 两条判据的**失效原因不同**，编号必须分开。
// ---------------------------------------------------------------------------

/// **场演化未真正发生**或**未写出**（`steps ≤ 0`／演化返回步数不符／`present_field` 写入数不符）。
pub const CODE_EVOLVE_NOT_WRITTEN: u8 = 129;
/// **场演化路径回读不一致**（写进去的字节读回来对不上）。
pub const CODE_EVOLVE_MISMATCH: u8 = 130;
/// **场演化或投影出口非法**（初始场采样不足／投影为空／规格非法）。
pub const CODE_EVOLVE_EMPTY: u8 = 131;
/// **图案退化**（演化后灰度种类 < [`EVOLVE_MIN_LEVELS`]）——**阳性对照**：全同色时
/// "偏移／字节序错误"根本测不出来 ⇒ 判据会**空转**，故必须显式判红。
pub const CODE_EVOLVE_PATTERN_DEGENERATE: u8 = 132;

// ---------------------------------------------------------------------------
// **退化出口**编号（133–134）→ CI 上读作 **233–234**
// ★ **R83**：退化 ⇔ "**没验到**"，与"**验到了**"必须给出**不同结果**。
// 旧写法退化返 `0`（＝全过）⇒ 静默绿。现映射为下述非零编号 ⇒ 判红。
// 分类本身在**纯算层**（[`classify_degenerate`]，机制 21），本层只做映射。
// ---------------------------------------------------------------------------

/// **帧缓冲不可用**（宽／高／行距为 `0`，或 `bpp` 越界）——**没验到**，判红。
pub const CODE_DEG_NO_FRAMEBUFFER: u8 = 133;
/// **可视区过小**（放不下该入口所需的图案）——**没验到**，判红。
pub const CODE_DEG_VIEWPORT_TOO_SMALL: u8 = 134;

/// 场演化网格边长：**`9 = 2³ + 1`**。
///
/// ★ **为什么必须是 `2^k + 1`**：多重网格的分层口径 `w_l = ((w−1) >> l) + 1`
/// 只在 `2^k + 1` 上逐层整除（**铁律**，`BadDims` 之外的尺寸会在最粗层露出错位）。
/// 本轮走的是 **B 路径（NCA 场演化）**，本身不限尺寸；选 `9` 是**为后续与 A1（多重网格）耦合留同一口径**
/// ＋ 把栈占用压到 ~7 KiB（见报告"边界说明"）。
pub const EVOLVE_GRID: usize = 9;
/// 场演化自检的**固定 PRNG 种子**（E1：确定性 ⇒ 同参同结果）。
pub const EVOLVE_RNG_SEED: u32 = 0x5EED_0920;
/// 图案**非退化**的下限：灰度种类数须 ≥ 此值（阳性对照的门线）。
pub const EVOLVE_MIN_LEVELS: usize = 8;

/// 一道平面（`CH` 通道 × `EVOLVE_GRID²` 格）。
const EVOLVE_PLANE: usize = nca::CH * EVOLVE_GRID * EVOLVE_GRID;
/// 感知缓冲（`PERCEPTION × w × h`）——**转调纯算层的同一算式**（不写死乘积，C19）。
const EVOLVE_PERC: usize = nca::PERCEPTION * EVOLVE_GRID * EVOLVE_GRID;
/// 单通道格点数。
const EVOLVE_CELLS: usize = EVOLVE_GRID * EVOLVE_GRID;

/// 把 `FrameBufferInfo` 的**数值面**交纯算层分类（**桥接在此，判定在 `nostd`**，机制 21）。
///
/// `min_w`／`min_h` ＝ 该入口所需的**最小可视区**（自检 `2×2`／product `4×4`／场演化 `9×9`）。
#[must_use]
fn degenerate_of(info: &FrameBufferInfo, buf_len: usize, min_w: usize, min_h: usize) -> Option<Degenerate> {
    classify_degenerate(
        info.width as usize,
        info.height as usize,
        info.stride as usize,
        info.bytes_per_pixel as usize,
        buf_len,
        min_w,
        min_h,
    )
}

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
/// ⚠️ **退化路径（R83 起：退化 ⇒ 判红，不再返回 0）**：帧缓冲不可用 ⇒ [`CODE_DEG_NO_FRAMEBUFFER`]；
/// 可视区放不下 2×2 ⇒ [`CODE_DEG_VIEWPORT_TOO_SMALL`]；`FbDesc` 与缓冲矛盾 ⇒ [`CODE_DESC_INCONSISTENT`]。
/// ★ **旧写法退化返 `0`** —— 而 `0` 是同一条"全过"的返回值 ⇒ **"没验到"会被读成"验过了"**（静默绿）。
/// 现改为**非零编号** ⇒ 调用方 `Fail(100 + r)` ⇒ **判红**。**不许把"没验到"读成"验过了"。**
///
/// ⚠️ **回读的适用边界（诚实写清）**：QEMU 的帧缓冲是**普通内存**，回读可靠；
/// **真实硬件上 WC（write-combining）显存回读可能拿到陈旧值** ⇒ 本判据的**适用范围仅 QEMU**。
#[must_use]
pub fn present_selfcheck(buf: &mut [u8], info: &FrameBufferInfo) -> u8 {
    // —— 退化出口**机器化**（R83）：分类在纯算层，本层只映射成编号 ——
    match degenerate_of(info, buf.len(), 2, 2) {
        None => {}
        Some(Degenerate::NoFramebuffer) => return CODE_DEG_NO_FRAMEBUFFER,
        Some(Degenerate::ViewportTooSmall) => return CODE_DEG_VIEWPORT_TOO_SMALL,
        Some(Degenerate::DescInconsistent) => return CODE_DESC_INCONSISTENT,
    }
    // 双保险：分类器已兜住"不可用"，此处若仍 `None` ⇒ 与分类器口径不一致 ⇒ 按**不可用**判红
    let Some(d) = desc_from_info(info) else {
        return CODE_DEG_NO_FRAMEBUFFER;
    };
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

/// **场演化路径自检**（**B 路径「低维场演化产生图像」的小范围验证**）：
/// **多步 NCA 演化 → `Project(S)` → 真实帧缓冲 → 回读逐字节比对**。
///
/// ## 与 [`present_product_selftest`]（126–128）的分工（**两条都要有**）
/// - 126–128 的场是**静态**的（SDF 一帧）：验"**接线 ＋ 回读**"，一旦通过即成**回归基线**；
/// - 本条（129–132）的场是**演化出来的**（[`nca::evolve`]）：验"**场演化 → 投影 → 像素**"这条链**真的通**。
///   ⇒ 静态那条**证不了"演化是否真的发生"**（恒等映射也能过）—— 这正是本条存在的理由。
///
/// ## 守约（逐条）
/// - **多重网格铁律**：网格 ＝ **`9 = 2³ + 1`**（[`EVOLVE_GRID`]）；为与 A1（多重网格）耦合留**同一口径**；
/// - **C1**：只用 `nostd` ＋ `bootloader_api`（**零第三方依赖**）；
/// - **C3／C9**：本文件**0 处 `unsafe`**（暂存全在栈上，不用 `alloc`）；
/// - **机制 21**：演化与投影在**纯算层**（`nostd::field::nca`／`nostd::project`），
///   本函数只做"**取硬件内存 → 写进去 → 读回来**"这件**边界动作**；
/// - **D8（呈现不增义）**：`Project(S)` 只做**线性投影 ＋ 钳位**，**无 gamma／无锐化／无调色**；
/// - **E1（确定性）**：`mask_p = 0.0` ＋ 固定种子 [`EVOLVE_RNG_SEED`] ⇒ **同参同结果**。
///
/// ## 判据（逐条，编号见常量）
/// 1. **演化必须真的发生**：`steps == 0` 或 `evolve` 返回步数 ≠ `steps` ⇒ [`CODE_EVOLVE_EMPTY`]／
///    [`CODE_EVOLVE_NOT_WRITTEN`] —— **不允许"不演化也算过"**（否则判据空转）；
/// 2. **写入必须足数**：`present_field` 返回 === `cells`，否则 [`CODE_EVOLVE_NOT_WRITTEN`]；
/// 3. **回读必须逐字节相等**：否则 [`CODE_EVOLVE_MISMATCH`]；
/// 4. **阳性对照**：演化后灰度**种类 ≥ [`EVOLVE_MIN_LEVELS`]**，否则
///    [`CODE_EVOLVE_PATTERN_DEGENERATE`]（全同色时"偏移／字节序错"根本暴露不出来）。
///
/// ## 栈占用（**如实标注**）
/// 暂存 ＝ `2×CH·81 ＋ PERCEPTION·81 ＋ HIDDEN ＋ 2×81` 个 `f32` ≈ **7 KiB**（全在栈上，不用堆）。
///
/// ⚠️ **回读适用边界**：同 [`present_selfcheck`] —— **仅在 QEMU 成立**
/// （真实硬件 WC 显存回读可能拿到陈旧值）。**本机无 QEMU** ⇒ 真实引导由 CI boot job 证成。
#[must_use]
pub fn present_evolve_selftest(buf: &mut [u8], info: &FrameBufferInfo, steps: u32) -> u8 {
    // —— 退化出口机器化（R83）：本入口所需最小可视区 ＝ `EVOLVE_GRID × EVOLVE_GRID` ——
    match degenerate_of(info, buf.len(), EVOLVE_GRID, EVOLVE_GRID) {
        None => {}
        Some(Degenerate::NoFramebuffer) => return CODE_DEG_NO_FRAMEBUFFER,
        Some(Degenerate::ViewportTooSmall) => return CODE_DEG_VIEWPORT_TOO_SMALL,
        Some(Degenerate::DescInconsistent) => return CODE_DESC_INCONSISTENT,
    }
    let Some(d) = desc_from_info(info) else {
        return CODE_DEG_NO_FRAMEBUFFER;
    };
    // **"不演化也算过"是不允许的** ⇒ 步数为 0 直接判红（调用方须给出 ≥1 步）
    if steps == 0 {
        return CODE_EVOLVE_EMPTY;
    }

    let n = EVOLVE_GRID;
    let cells = EVOLVE_CELLS;

    // —— 暂存全部在栈上（~7 KiB；不用 `alloc` ⇒ 不碰 4 KiB 单帧分配上限）——
    let mut a = [0.0f32; EVOLVE_PLANE];
    let mut b = [0.0f32; EVOLVE_PLANE];
    let mut percept = [0.0f32; EVOLVE_PERC];
    let mut hidden = [0.0f32; nca::HIDDEN];
    let mut pool = [0.0f32; EVOLVE_CELLS];
    let mut alive = [0.0f32; EVOLVE_CELLS];
    let mut seed_field = [0.0f32; EVOLVE_CELLS];

    // —— 初始状态：SDF 圆盘，**几何由帧缓冲导出**（D8：呈现＝内核状态的直接投影）——
    let cx = (n as f32 - 1.0) * 0.5;
    let cy = cx;
    let r = n as f32 * 0.5 - 0.5;
    let m = sdf::sample_into(|x, y| sdf::circle(cx, cy, r, x, y), n, n, &mut seed_field);
    if m != cells {
        return CODE_EVOLVE_EMPTY;
    }
    // 每个通道都填同一初始状态：圆内 `v ∈ (0,1]`、圆外 `0`（通道 0 = alpha ⇒ 决定存活掩码）
    let mut c = 0usize;
    while c < nca::CH {
        let mut i = 0usize;
        while i < cells {
            let t = -seed_field[i] / r; // SDF：圆内为负 ⇒ 取负得正
            a[c * cells + i] = if t > 0.0 {
                if t > 1.0 {
                    1.0
                } else {
                    t
                }
            } else {
                0.0
            };
            i += 1;
        }
        c += 1;
    }

    // —— ★ 场演化（纯算层；`mask_p = 0.0` ⇒ 不丢格；固定种子 ⇒ 确定性）——
    let weights = nca::forward_const_weights();
    let mut rng = nca::Xorshift32::new(EVOLVE_RNG_SEED);
    let done = nca::evolve(
        &mut a, &mut b, n, n, &weights, &mut rng, 0.0,
        &mut percept, &mut hidden, &mut pool, &mut alive, steps,
    );
    if done != steps {
        return CODE_EVOLVE_NOT_WRITTEN; // 演化没走完 ⇒ 不许当成功
    }

    // —— 投影 ＋ 写入真实帧缓冲（**product 入口**；通道 0 = alpha 平面）——
    let spec = match ProjectSpec::new(n, n, 0.0, 1.0) {
        Some(s) => s,
        None => return CODE_EVOLVE_EMPTY,
    };
    let written = present_field(buf, info, &a[..cells], &spec);
    if written != cells {
        return CODE_EVOLVE_NOT_WRITTEN;
    }

    // —— 回读：与 `fb::encode` 的期望字节逐字节比对（期望值从被测物推导，D40／C19）——
    let gray = project::project_gray_u8(&a[..cells], &spec);
    if gray.len() != cells {
        return CODE_EVOLVE_EMPTY;
    }
    let mut i = 0usize;
    while i < cells {
        let x = i % n;
        let y = i / n;
        let g = gray[i];
        let (bytes, bn) = fb::encode(&d, Rgb24::new(g, g, g));
        if bn == 0 {
            return CODE_FORMAT_UNSUPPORTED;
        }
        let Some(off) = d.offset_of(x, y) else {
            return CODE_EVOLVE_MISMATCH;
        };
        let Some(end) = off.checked_add(bn) else {
            return CODE_EVOLVE_MISMATCH;
        };
        if end > buf.len() {
            return CODE_EVOLVE_MISMATCH;
        }
        let mut k = 0usize;
        while k < bn {
            if buf[off + k] != bytes[k] {
                return CODE_EVOLVE_MISMATCH;
            }
            k += 1;
        }
        i += 1;
    }

    // —— ★ 阳性对照（**就地可判**）：图案必须**非退化** ——
    // 全同色时"偏移／步长／字节序错误"**根本暴露不出来**（回读会照样一致）⇒ 判据空转。
    // 判法：数灰度种类（**不用 `alloc`**：用 256 桶计数 ＋ 只数到门线即可提前返回）。
    let mut seen = [false; 256];
    let mut levels = 0usize;
    let mut j = 0usize;
    while j < cells {
        let g = gray[j] as usize;
        if !seen[g] {
            seen[g] = true;
            levels += 1;
            if levels >= EVOLVE_MIN_LEVELS {
                return 0; // 已达标 ⇒ 全过
            }
        }
        j += 1;
    }
    CODE_EVOLVE_PATTERN_DEGENERATE
}

/// **product 路径自检**：**真的把 [`present_field`]（产品入口）跑一遍**，并回读校验。
///
/// ## 为什么需要它（= 2.4「product 路径接线」）
/// [`present_selfcheck`]（121–125）验的是**一块写死的 2×2 图案** —— 它证明"桥接与写入正确"，
/// 但**不经过 `present_field`** ⇒ **产品入口此前在裸机上从未被调用过**
/// （编译期 `function present_field is never used` 警告即是证据，见 `reports/`）。
/// ⇒ 本函数把 **`present_field` 挂进真实调用路径**：场 → `Project(S)` → 真实帧缓冲，
/// 并**回读校验**（返回值只能证明"没被拒绝"，证不了"字节真落在那块内存上"）。
///
/// ## 场从哪来（D8：**呈现 ＝ 内核状态的直接投影 —— 界面就是它自己**）
/// 场源 ＝ **纯算层 `field::sdf`** 对**帧缓冲几何**（`info.width/height`）采样得到的圆盘 SDF：
/// 圆心／半径**由显示几何导出** ⇒ 投影像**确实是"当前状态"的函数**，而非写死图案。
/// ⚠️ **诚实标注**：这是 2.4 阶段的**最小 product 路径**；下一步（2.4 × L6 集成）
/// 将把场源换成**场演化产生的图像**（B 路径 / NCA），**本函数的接线与校验形态不变**。
///
/// ## 判据（逐条）
/// - `present_field` 返回的**写入数须 == `w*h`**，否则 [`CODE_PRODUCT_NOT_WRITTEN`]；
/// - 回读须与 `fb::encode` 的**期望字节逐字节相等**，否则 [`CODE_PRODUCT_MISMATCH`]；
/// - 场采样／投影长度为 0 ⇒ [`CODE_PRODUCT_EMPTY`]。
///
/// ⚠️ **退化路径（R83 起：退化 ⇒ 判红）**：帧缓冲不可用 ⇒ [`CODE_DEG_NO_FRAMEBUFFER`]；
/// 可视区小于 4×4 ⇒ [`CODE_DEG_VIEWPORT_TOO_SMALL`]；描述与缓冲矛盾 ⇒ [`CODE_DESC_INCONSISTENT`]。
/// ⚠️ **回读适用边界**：同 [`present_selfcheck`] —— **仅在 QEMU 成立**
/// （真实硬件 WC 显存回读可能拿到陈旧值）。
#[must_use]
pub fn present_product_selftest(buf: &mut [u8], info: &FrameBufferInfo) -> u8 {
    match degenerate_of(info, buf.len(), 4, 4) {
        None => {}
        Some(Degenerate::NoFramebuffer) => return CODE_DEG_NO_FRAMEBUFFER,
        Some(Degenerate::ViewportTooSmall) => return CODE_DEG_VIEWPORT_TOO_SMALL,
        Some(Degenerate::DescInconsistent) => return CODE_DESC_INCONSISTENT,
    }
    let Some(d) = desc_from_info(info) else {
        return CODE_DEG_NO_FRAMEBUFFER;
    };
    // 图案尺寸：16×16，但**不得越出真实可视区**（`offset_of` 在 x≥width 时返回 None）
    let w = if info.width < 16 { info.width } else { 16 };
    let h = if info.height < 16 { info.height } else { 16 };
    if w < 4 || h < 4 {
        return CODE_DEG_VIEWPORT_TOO_SMALL; // 防御（分类器已兜住 ⇒ 双保险，同口径）
    }
    match desc_min_len(&d) {
        Some(n) if n <= buf.len() => {}
        _ => return CODE_DESC_INCONSISTENT,
    }

    // —— 场源：纯算层 SDF，参数由**帧缓冲几何**导出（D8：状态 → 像素）——
    let mut field = [0.0f32; 256];
    let cells = w * h;
    let cx = (w as f32 - 1.0) * 0.5;
    let cy = (h as f32 - 1.0) * 0.5;
    let r = (if w < h { w } else { h }) as f32 * 0.5 - 1.0;
    let n = sdf::sample_into(|x, y| sdf::circle(cx, cy, r, x, y), w, h, &mut field[..cells]);
    if n != cells {
        return CODE_PRODUCT_EMPTY;
    }
    let spec = match ProjectSpec::new(w, h, -r, r) {
        Some(s) => s,
        None => return CODE_PROJECT_INVALID,
    };

    // —— ★ product 路径：**调用 `present_field`（本函数真被 `run()` 调用 ⇒ 产品入口已接线）** ——
    let written = present_field(buf, info, &field[..cells], &spec);
    if written != cells {
        return CODE_PRODUCT_NOT_WRITTEN;
    }

    // —— 回读：与 `fb::encode` 的期望字节逐字节比对（**期望值从被测物推导，不手写**，D40／C19）——
    let gray = project::project_gray_u8(&field[..cells], &spec);
    if gray.len() != cells {
        return CODE_PRODUCT_EMPTY;
    }
    let mut i = 0usize;
    while i < cells {
        let x = i % w;
        let y = i / w;
        let g = gray[i];
        let (bytes, bn) = fb::encode(&d, Rgb24::new(g, g, g));
        if bn == 0 {
            return CODE_FORMAT_UNSUPPORTED;
        }
        let Some(off) = d.offset_of(x, y) else {
            return CODE_PRODUCT_MISMATCH;
        };
        let Some(end) = off.checked_add(bn) else {
            return CODE_PRODUCT_MISMATCH;
        };
        if end > buf.len() {
            return CODE_PRODUCT_MISMATCH;
        }
        let mut k = 0usize;
        while k < bn {
            if buf[off + k] != bytes[k] {
                return CODE_PRODUCT_MISMATCH;
            }
            k += 1;
        }
        i += 1;
    }

    0
}
