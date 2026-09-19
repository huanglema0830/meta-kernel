//! # 1.3 二维场投影 `Project(S)`（**纯算层**）
//!
//! **★ D8 裁定（2026-09-20）**：**呈现 ＝ 内核状态的直接投影 —— 界面就是它自己**。
//! **没有渲染管线**，只有 `Project(S)`：`S`（内核状态）→ 像素，**一步到位**；
//! **不引入外部素材、不引入美术规则、不引入中间表示**（无场景图／无图层／无着色器）。
//!
//! ## 守约（三条）
//! 1. **机制 21（执行体归属）**：本模块**只把状态算成像素数据并"交出去"**；
//!    **不持有、也不写入**帧缓冲（"取硬件内存并改写它"属**动作** ⇒ 只能由**边界层**
//!    （宿主／boot 层）做，见 `meta-kernel-boot/kernel/src/present.rs`）。
//! 2. **C1**（零第三方依赖）｜**C3/C9**（零 `unsafe`）。
//! 3. **D8 的"不增义"**：本模块**不得**做任何"美化"—— 无插值锐化、无调色、无 gamma 曲线；
//!    **只有线性映射 + 钳位**。任何"让画面更好看"的加工都属**增义**，须另案裁定。
//!
//! ## 为什么返回 `Vec<u8>` 而不是写 `&mut [u8]`
//! 写入 `&mut [u8]` 缓冲＝**持有并改写**一块内存（即便它不是硬件帧缓冲，判据也难以区分）
//! ⇒ 按机制 21「★ 帧缓冲补充口径」，纯算层**交出数据**即可，**拷贝由边界层做**。

use crate::fb::Rgb24;
use crate::field::gray_to_rgb24;
use crate::math::clamp01;
use alloc::vec::Vec;

/// 投影规格（**纯数据**）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectSpec {
    /// 宽（格点数）。
    pub w: usize,
    /// 高（格点数）。
    pub h: usize,
    /// 映射下界（≤ lo 的场值 ⇒ 黑）。
    pub lo: f32,
    /// 映射上界（≥ hi 的场值 ⇒ 白）。
    pub hi: f32,
}

impl ProjectSpec {
    /// 构造。`w`/`h` 必须 > 0；`hi` 必须 **严格大于** `lo`（相等 ⇒ 除零 ⇒ 拒绝）。
    #[must_use]
    pub fn new(w: usize, h: usize, lo: f32, hi: f32) -> Option<Self> {
        if w == 0 || h == 0 || !(hi > lo) {
            return None;
        }
        Some(Self { w, h, lo, hi })
    }

    /// 格点数 `w × h`（溢出返回 `None`）。
    #[must_use]
    pub fn cells(&self) -> Option<usize> {
        self.w.checked_mul(self.h)
    }
}

/// **把场值线性映射到 `[0,1]`**（钳位）。
///
/// ⚠️ **这是 `Project(S)` 的全部"美术"**：一步线性归一化。没有 gamma、没有直方图均衡。
#[must_use]
pub fn normalize(v: f32, lo: f32, hi: f32) -> f32 {
    if !(hi > lo) {
        return 0.0;
    }
    clamp01((v - lo) / (hi - lo))
}

/// **`Project(S)` 的灰度形态**：场 → 每格点 1 字节（`0..=255`）。
///
/// 长度不足／规格非法 ⇒ 返回**空** `Vec`（**绝不**返回部分结果冒充成功，C5）。
#[must_use]
pub fn project_gray_u8(field: &[f32], spec: &ProjectSpec) -> Vec<u8> {
    let Some(n) = spec.cells() else {
        return Vec::new();
    };
    if field.len() < n {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n);
    for &v in field.iter().take(n) {
        // 255 是 u8 上界；`+ 0.5` 为四舍五入（不是"美化"，是量化必需）。
        out.push((normalize(v, spec.lo, spec.hi) * 255.0 + 0.5) as u8);
    }
    out
}

/// **`Project(S)` 的 RGB24 形态**：场 → 每格点 3 字节（`R,G,B` 各 8 位，灰度）。
///
/// 返回长度 = `w × h × 3`。规格非法 ⇒ 空 `Vec`。
#[must_use]
pub fn project_rgb24(field: &[f32], spec: &ProjectSpec) -> Vec<u8> {
    let Some(n) = spec.cells() else {
        return Vec::new();
    };
    if field.len() < n {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n * 3);
    for &v in field.iter().take(n) {
        let g = (normalize(v, spec.lo, spec.hi) * 255.0 + 0.5) as u8;
        let Rgb24 { r, g: gg, b } = gray_to_rgb24(g);
        out.push(r);
        out.push(gg);
        out.push(b);
    }
    out
}

/// **把已算好的 RGB24 序列拷进目标切片**（**边界层用**；本函数仍在纯算层，
/// 因为它只做按字节拷贝，不"取"任何硬件内存）。返回写入的字节数。
pub fn copy_rgb24_into(src: &[u8], dst: &mut [u8]) -> usize {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ① **规格校验**：`w=0`／`h=0`／`hi ≤ lo` 一律拒绝（除零防护）。
    #[test]
    fn spec_rejects_degenerate() {
        assert!(ProjectSpec::new(0, 4, 0.0, 1.0).is_none());
        assert!(ProjectSpec::new(4, 0, 0.0, 1.0).is_none());
        assert!(ProjectSpec::new(4, 4, 1.0, 1.0).is_none(), "hi==lo ⇒ 除零 ⇒ 拒绝");
        assert!(ProjectSpec::new(4, 4, 2.0, 1.0).is_none(), "hi<lo ⇒ 拒绝");
        assert!(ProjectSpec::new(4, 4, 0.0, 1.0).is_some());
    }

    /// ② **常数场 ⇒ 全屏同色**（"界面就是它自己"的最基本形态）。
    #[test]
    fn constant_field_is_uniform() {
        let spec = ProjectSpec::new(4, 4, 0.0, 1.0).expect("spec");
        let field = [0.5f32; 16];
        let px = project_gray_u8(&field, &spec);
        assert_eq!(px.len(), 16);
        assert!(px.iter().all(|&v| v == px[0]), "常数场必须全屏同色");
        assert_eq!(px[0], 128, "0.5 落在 [0,1] 中点 ⇒ 128（±1 量化）");
    }

    /// ③ **钳位**：越界场值不得绕回（wrap-around 会画出假图案）。
    #[test]
    fn out_of_range_clamps() {
        let spec = ProjectSpec::new(2, 1, 0.0, 1.0).expect("spec");
        let px = project_gray_u8(&[-5.0f32, 5.0], &spec);
        assert_eq!(px, alloc::vec![0u8, 255u8], "下越界⇒黑，上越界⇒白，不得绕回");
    }

    /// ④ **单调性**（阳性对照）：场值越大 ⇒ 像素越亮（**不得**出现反相）。
    #[test]
    fn monotone_brightness() {
        let spec = ProjectSpec::new(4, 1, 0.0, 1.0).expect("spec");
        let field = [0.1f32, 0.4, 0.6, 0.9];
        let px = project_gray_u8(&field, &spec);
        for w in px.windows(2) {
            assert!(w[0] <= w[1], "亮度必须随场值单调不减：{:?}", px);
        }
    }

    /// ⑤ **RGB24 长度**＝格点数 × 3，且灰度三通道相等。
    #[test]
    fn rgb24_shape() {
        let spec = ProjectSpec::new(3, 2, 0.0, 1.0).expect("spec");
        let field = [0.5f32; 6];
        let px = project_rgb24(&field, &spec);
        assert_eq!(px.len(), 18);
        for c in px.chunks(3) {
            assert_eq!(c[0], c[1]);
            assert_eq!(c[1], c[2]);
        }
    }

    /// ⑥ **输入不足 ⇒ 空 Vec**（不返回部分结果冒充成功）。
    #[test]
    fn short_input_yields_empty() {
        let spec = ProjectSpec::new(4, 4, 0.0, 1.0).expect("spec");
        assert!(project_gray_u8(&[0.5f32; 3], &spec).is_empty());
    }

    /// ⑦ **拷贝**：按 `min(len)` 截断，返回实际字节数。
    #[test]
    fn copy_truncates_to_min() {
        let src = [1u8, 2, 3, 4];
        let mut dst = [0u8; 2];
        assert_eq!(copy_rgb24_into(&src, &mut dst), 2);
        assert_eq!(dst, [1, 2]);
    }
}
