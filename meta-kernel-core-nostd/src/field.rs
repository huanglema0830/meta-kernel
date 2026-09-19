//! # 场方程求解器 · **纯算层根模块**（阶段二 2.4 · 技术路径 **A1 ＋ B 并行**）
//!
//! 来源：`coordination/discussions/2026-09-19_场方程求解器设计稿.md`
//! §2.2.1（背景）、§2.2.2（数学基础）、§2.2.4（劈分）、§2.2.5（与现有模块的关系）。
//!
//! ## A1 与 B 的分工（用户 2026-09-19 裁定）
//!
//! | 路径 | 内容 | 本模块的对应 |
//! |---|---|---|
//! | **A1** | CPU SIMD ＋ **多重网格场方程求解** | [`poisson`] |
//! | **B** | **低维场演化产生图像** | [`nca`] ＋ 本文件的投影函数 |
//!
//! **两者共用同一个核心** ＝ 场方程求解器（`NCA` 为核心方案）。⇒ 本模块就是那个共用核心。
//!
//! ## 纯算层与边界层的劈分（设计稿 §2.2.4）
//!
//! ```text
//! ┌─ 纯算层（本 crate｜零依赖｜零 unsafe｜host 可 100% 单测）───────────────┐
//! │ §1 场方程求解   poisson::solve_poisson_mg   ∇²V = ρ 的多重网格 V-cycle │
//! │ §2 NCA 局部规则 nca::step（Sobel＋MLP 前向＋确定性掩码＋存活掩码）       │
//! │ §3 SDF 计算     sdf::*（解析几何 ＋ 并交差）                            │
//! │ §4 投影函数     project_u8（**唯一的"格式"出口**）                      │
//! │ §5 复值场运算    ComplexField2D（与 `interference` 同源，C19）           │
//! │ §6 时间节律      rhythm_steps（**import** `engine_select`，不改场）      │
//! │ ── 出口：粗网格 RGB/灰度缓冲 ──                                        │
//! └────────────────────────────────────────────────────────────────────────┘
//!                    ↓（同一块内存，**boot 层**借用；写入属边界行为）
//! ┌─ 边界层（`meta-kernel-boot/kernel/src/`）｜可零 unsafe ────────────────┐
//! │ §7 轻量解码器  fb::upscale_*（**算法**在纯算层，"写到哪块内存"是边界）    │
//! │ §8 写入帧缓冲  fb::put_pixel / fill_rect / blit                        │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 与既有模块的关系（设计稿 §2.2.5；**只 `import`，不重实现** —— C19）
//!
//! | 既有模块 | 关系 |
//! |---|---|
//! | [`crate::engine_select`]（Q11） | **刷新节律**由三引擎决定 ⇒ [`rhythm_steps`] **import** 它，不改场 |
//! | [`crate::interference`] | **相位差**只有一个口径 ⇒ [`phase_difference_from_samples`] **直接转发**干扰模块的实现 |
//! | [`crate::fmath`] | 所有超越函数（`sqrt`/`exp`/`sin`/`cos`）**只用它**（`core` 无这些方法） |
//! | `crate::fb`（本 crate 新增） | 投影输出 → 像素字节的**格式出口** |
//!
//! ## 明确**不做**（防越权）
//!
//! - ❌ **不在内核内训练**（E2/C1）：本层只做**前向**；权重以 `const` 表进内核。
//! - ❌ **不做 Fourier-NCA**：需 FFT，而 `fmath` **没有 FFT**（设计稿 §2.2.2(5) 的 F1）
//!   ⇒ 属阶段 2 的增量，**须先裁定**。
//! - ❌ **不做 L4/L5 接线**：设计稿 §2.2.5(2) 的 L4/L5 关系是**口径**（"可被 L4 式拒绝／L5 式诊断"），
//!   直接接 `l4_gate`／`l5_diagnosis` 会造成**语义污染** ⇒ 需先裁定（Q7）。

use crate::energy::EnergyPool;
use crate::engine_select::{self, Engine};
use crate::fmath;
use crate::interference;
use crate::math::clamp01;

pub mod nca;
pub mod poisson;
pub mod sdf;

/// 粗网格格距（**无量纲**；设计稿 §2.2.2(2) 的 `ε` 规则以它为单位）。
pub const COARSE_GRID_STEP: f32 = 1.0;

// ===================== §3.5 ρ(SDF) —— 源项 =====================

/// `tanh` 的**稳定**实现：`tanh(u) = 1 − 2/(e^{2u}+1)`。
///
/// **为什么不用库函数**：`core` 无 `tanh`（`fmath` 也未提供）⇒ 用 [`fmath::exp`] 组合。
/// 大 `|u|` 时 `exp` 溢出为 `inf` ⇒ 结果正确退化为 `±1`（**不产生 NaN**，已测）。
#[must_use]
pub fn tanh_stable(u: f32) -> f32 {
    if u.is_nan() {
        return f32::NAN;
    }
    let e = fmath::exp(2.0 * u);
    if e.is_infinite() {
        return 1.0;
    }
    1.0 - 2.0 / (e + 1.0)
}

/// **软阈值源项**（用户裁定 Q3 ＝ **乙**）：`ρ = −tanh(SDF/ε)`。
///
/// **符号含义**：`SDF < 0`（物体**内部**）⇒ `ρ > 0` ⇒ **内部产生正源**
/// ⇒ 解出 `V > 0` 于内部、`V = 0` 于边框 ⇒ **零等值面落在内部与边框之间**（即"可见边界"所在）。
///
/// **为什么必须软**（设计稿 §2.2.2(2)）：`δ` 函数式薄壳源会把**高频分量**注入粗网格，
/// 粗网格表示不了 ⇒ **多重网格收敛退化**。`tanh` 光滑、无奇异。
#[must_use]
pub fn rho_soft_threshold(sdf: f32, eps: f32) -> f32 {
    if eps <= 0.0 {
        // 退化：ε→0 时 soft 退化为 ±1 的阶跃（**不 panic、不除零**）
        return if sdf > 0.0 { -1.0 } else if sdf < 0.0 { 1.0 } else { 0.0 };
    }
    -tanh_stable(sdf / eps)
}

/// 对照用的**硬截断**版本（设计稿 §2.2.2(2) 候选乙的另一种写法）。
///
/// **仅供对照/测试**，**不是选定口径**（选定的是 [`rho_soft_threshold`]）。
#[must_use]
pub fn rho_clamp(sdf: f32, eps: f32) -> f32 {
    if eps <= 0.0 {
        return rho_soft_threshold(sdf, eps);
    }
    -(sdf / eps).clamp(-1.0, 1.0)
}

/// `ε` 的取值规则（用户裁定）：**`ε ≥ 1.5 × 粗网格格距`**。
///
/// **为什么绑格距**：否则源项在最粗一层上是**冲激**（宽度 < 1 格 ⇒ 粗网格采不到）。
#[must_use]
pub fn eps_min(coarse_step: f32) -> f32 {
    1.5 * coarse_step
}

// ===================== 场容器 =====================

/// **二维标量场**（`V`）：`w × h`，行优先，**借用调用方内存**（本层不 `alloc`）。
#[derive(Debug)]
pub struct Field2D<'a> {
    /// 宽（格数）
    pub w: usize,
    /// 高（格数）
    pub h: usize,
    /// 数据（长度 ≥ `w*h`）
    pub data: &'a mut [f32],
}

impl<'a> Field2D<'a> {
    /// 缓冲不足 ⇒ `None`（**不 panic**）。
    #[must_use]
    pub fn new(w: usize, h: usize, data: &'a mut [f32]) -> Option<Self> {
        if w == 0 || h == 0 || data.len() < w * h {
            return None;
        }
        Some(Self { w, h, data })
    }

    #[must_use]
    pub fn cells(&self) -> usize {
        self.w * self.h
    }

    #[inline]
    #[must_use]
    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    #[must_use]
    pub fn get(&self, x: usize, y: usize) -> Option<f32> {
        if x >= self.w || y >= self.h {
            return None;
        }
        Some(self.data[self.idx(x, y)])
    }

    pub fn set(&mut self, x: usize, y: usize, v: f32) -> bool {
        if x >= self.w || y >= self.h {
            return false;
        }
        let i = self.idx(x, y);
        self.data[i] = v;
        true
    }

    pub fn fill(&mut self, v: f32) {
        let n = self.cells();
        for d in self.data[..n].iter_mut() {
            *d = v;
        }
    }

    /// **Dirichlet `V = 0` 边框**（设计稿 §2.2.2(4) 的选定口径）；返回被钉死的**不同格点数**。
    pub fn dirichlet_zero_border(&mut self) -> usize {
        let (w, h) = (self.w, self.h);
        if w == 0 || h == 0 {
            return 0;
        }
        let mut n = 0usize;
        for x in 0..w {
            self.data[x] = 0.0;
            self.data[(h - 1) * w + x] = 0.0;
            // `h == 1` 时"上"与"下"是同一格 ⇒ 只算一次
            n += if h == 1 { 1 } else { 2 };
        }
        for y in 1..h.saturating_sub(1) {
            self.data[y * w] = 0.0;
            self.data[y * w + w - 1] = 0.0;
            // `w == 1` 时"左"与"右"是同一格
            n += if w == 1 { 1 } else { 2 };
        }
        n
    }

    /// **未缩放的五点 Laplacian** `Σ邻居 − 4V`（`h = 1`）；边框 ⇒ `None`。
    #[must_use]
    pub fn laplacian_at(&self, x: usize, y: usize) -> Option<f32> {
        if x == 0 || y == 0 || x + 1 >= self.w || y + 1 >= self.h {
            return None;
        }
        let i = self.idx(x, y);
        let d = &self.data;
        Some(d[i - 1] + d[i + 1] + d[i - self.w] + d[i + self.w] - 4.0 * d[i])
    }

    /// 内部点的 `max|V|`（边框不计）。
    #[must_use]
    pub fn max_abs_interior(&self) -> f32 {
        if self.w < 3 || self.h < 3 {
            return 0.0;
        }
        let mut m = 0.0f32;
        for y in 1..self.h - 1 {
            for x in 1..self.w - 1 {
                let a = fmath::abs_f32(self.data[self.idx(x, y)]);
                if a > m {
                    m = a;
                }
            }
        }
        m
    }

    /// **用户扰动注入接口**（设计稿：用户操作 ＝ 场扰动）：在 `(x, y)` 处加 `amp`。
    pub fn perturb(&mut self, x: usize, y: usize, amp: f32) -> bool {
        if x >= self.w || y >= self.h {
            return false;
        }
        let i = self.idx(x, y);
        self.data[i] += amp;
        true
    }

    /// 高斯扰动（**确定性**、纯算）：在 `(cx, cy)` 附近注入 `amp·e^{−(d²/2σ²)}`；返回注入格点数。
    ///
    /// **为什么用高斯而非 `δ`**：`δ` 会注入高频 ⇒ 与"为什么 ρ 要软阈值"同一条理由。
    pub fn perturb_gaussian(&mut self, cx: f32, cy: f32, sigma: f32, amp: f32) -> usize {
        if sigma <= 0.0 || self.w == 0 || self.h == 0 {
            return 0;
        }
        let inv2s2 = 1.0 / (2.0 * sigma * sigma);
        let mut n = 0usize;
        for y in 0..self.h {
            for x in 0..self.w {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let v = amp * fmath::exp(-(dx * dx + dy * dy) * inv2s2);
                let i = self.idx(x, y);
                self.data[i] += v;
                n += 1;
            }
        }
        n
    }

    #[must_use]
    pub fn as_slice(&self) -> &[f32] {
        &self.data[..self.cells()]
    }

    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        let n = self.cells();
        &mut self.data[..n]
    }
}

// ===================== §4 投影函数（唯一的"格式"出口） =====================

/// **线性投影**：`v ↦ clamp01((v − lo)/(hi − lo)) ↦ u8`。
///
/// ★ **"呈现不增义"的实现保证**：本函数**是纯函数**，**只做仿射映射 ＋ 量化**
/// —— **不做**色调映射、不做 gamma、不做边缘增强、不做任何"更好看"的处理。
/// 要加任何修饰 ⇒ 必须先裁定（属"增义"）。
///
/// `hi <= lo` ⇒ 视为退化区间，全部映射为 0（**不 panic、不除零**）。
pub fn project_u8(field: &[f32], lo: f32, hi: f32, out: &mut [u8]) -> usize {
    let n = field.len().min(out.len());
    let span = hi - lo;
    let mut i = 0usize;
    while i < n {
        let t = if span > 0.0 { clamp01((field[i] - lo) / span) } else { 0.0 };
        out[i] = (t * 255.0 + 0.5) as u8;
        i += 1;
    }
    n
}

/// 把灰度投影结果**展开成 `Rgb24`**（等值三通道；**不引入任何色彩关系**）。
#[must_use]
pub fn gray_to_rgb24(g: u8) -> crate::fb::Rgb24 {
    crate::fb::Rgb24::new(g, g, g)
}

// ===================== §5 复值场（幅度 ＋ 相位） =====================

/// **复值场** `Ψ = A·e^{iθ}`，存为两条实数组（SoA，设计稿 §2.2.3(6)）。
///
/// **为什么复值**：相位能表达"节律/干涉"，与既有 [`crate::interference`] 模块**直接同构**。
/// **代价**：存储 ×2、算力 ×2～3（设计稿已登记）。
#[derive(Debug)]
pub struct ComplexField2D<'a> {
    /// 宽
    pub w: usize,
    /// 高
    pub h: usize,
    /// 幅度 `A`
    pub amp: &'a mut [f32],
    /// 相位 `θ`（弧度；约定归一化到 `[0, 2π)`）
    pub phase: &'a mut [f32],
}

impl<'a> ComplexField2D<'a> {
    #[must_use]
    pub fn new(w: usize, h: usize, amp: &'a mut [f32], phase: &'a mut [f32]) -> Option<Self> {
        if w == 0 || h == 0 || amp.len() < w * h || phase.len() < w * h {
            return None;
        }
        Some(Self { w, h, amp, phase })
    }

    #[must_use]
    pub fn cells(&self) -> usize {
        self.w * self.h
    }

    /// 清空（`A = 0`、`θ = 0`）。
    pub fn clear(&mut self) {
        let n = self.cells();
        for v in self.amp[..n].iter_mut() {
            *v = 0.0;
        }
        for v in self.phase[..n].iter_mut() {
            *v = 0.0;
        }
    }

    /// **叠加平面波** `A += a`、`θ += kx·x + ky·y`（相位 `rem_euclid(2π)` 归一）；返回格点数。
    pub fn deposit_plane_wave(&mut self, kx: f32, ky: f32, a: f32) -> usize {
        let n = self.cells();
        for y in 0..self.h {
            for x in 0..self.w {
                let i = y * self.w + x;
                self.amp[i] += a;
                self.phase[i] = fmath::rem_euclid_f32(self.phase[i] + kx * x as f32 + ky * y as f32, core::f32::consts::TAU);
            }
        }
        n
    }

    /// 强度 `|Ψ|² = A²`（**只做平方**，无增义）。
    pub fn intensity_into(&self, out: &mut [f32]) -> usize {
        let n = self.cells().min(out.len());
        for i in 0..n {
            out[i] = self.amp[i] * self.amp[i];
        }
        n
    }
}

/// **显式相位差** `θₐ[i] − θ_b[i]`（两个**自带相位**的场之比）。
///
/// ⚠️ **概念分离（C19）**：本函数与 [`phase_difference_from_samples`] 是**两个不同的概念**，
/// 不可互相顶替：
/// - 本函数 = **显式相位**之差（场本身带 `θ`）；
/// - 后者 = **由样点谱提取**的相位之差（`interference` 口径，需要 ≥8 点 + DFT）。
/// ⇒ 二者**各有各的判据**，不要"统一"成一个（那会掩盖两者前提不同的事实）。
#[must_use]
pub fn phase_difference_explicit(ta: &[f32], tb: &[f32], i: usize) -> Option<f32> {
    if i >= ta.len() || i >= tb.len() {
        return None;
    }
    Some(ta[i] - tb[i])
}

/// **由样点谱提取的相位差** —— **直接转发** [`crate::interference::phase_difference`]。
///
/// ★ 本函数**不重新实现**任何逻辑（C19 同源）：同一个概念只许**一份**实现。
/// ⇒ 它的存在只是为了"把名字摆在场的语境里"，**语义与 `interference` 逐位一致**（已测）。
#[must_use]
pub fn phase_difference_from_samples(a: &[f32], b: &[f32]) -> f32 {
    interference::phase_difference(a, b)
}

/// **复值场叠加** `Ψ_c = Ψ_a + Ψ_b`（逐点复数相加）：把结果写进 `out_amp`／`out_phase`；返回格点数。
///
/// 复数加法需要 `cos`/`sin` ⇒ **用 [`crate::fmath`]**（`core` 无这些方法）。
pub fn superpose(
    a_amp: &[f32],
    a_phase: &[f32],
    b_amp: &[f32],
    b_phase: &[f32],
    out_amp: &mut [f32],
    out_phase: &mut [f32],
) -> usize {
    let n = a_amp
        .len()
        .min(a_phase.len())
        .min(b_amp.len())
        .min(b_phase.len())
        .min(out_amp.len())
        .min(out_phase.len());
    let mut i = 0usize;
    while i < n {
        let re = a_amp[i] * fmath::cos(a_phase[i]) + b_amp[i] * fmath::cos(b_phase[i]);
        let im = a_amp[i] * fmath::sin(a_phase[i]) + b_amp[i] * fmath::sin(b_phase[i]);
        // |Ψ| 需要 sqrt（core 无 ⇒ 用 fmath）
        out_amp[i] = fmath::sqrt(re * re + im * im);
        out_phase[i] = fmath::rem_euclid_f32(fmath::atan2(im, re), core::f32::consts::TAU);
        i += 1;
    }
    n
}

// ===================== §6 时间节律（import engine_select；**不改场**） =====================

/// **刷新节律**：调用 [`crate::engine_select`] 的三引擎，把结果映射成"本轮推进几步"。
///
/// ★ **本函数不改场**（设计稿 §2.2.4 的 §6 口径）：只返回 `(引擎, 步数)`，
/// 由调用方决定拿它去推 `nca::step` 几次。⇒ **节律与场演化解耦**（可分别测）。
///
/// 映射（**写死、可判**）：`steps = 1 + ⌊clamp01(x)·base⌋`，`x` 为引擎对 `seed` 的一步输出。
/// ⇒ 保证 `steps ∈ [1, base+1]`（**永不为 0**，否则"不推进"会让判据假绿）。
#[must_use]
pub fn rhythm_steps(pool: &EnergyPool, seed: f32, base: u32) -> (Engine, u32) {
    let (e, x) = engine_select::select_and_step(pool, seed);
    let t = clamp01(x);
    let steps = 1 + (t * base as f32) as u32;
    (e, steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_of(w: usize, h: usize, v: f32) -> Vec<f32> {
        vec![v; w * h]
    }

    // ---------- ρ(SDF) ----------

    #[test]
    fn tanh_stable_extremes_and_no_nan() {
        assert!((tanh_stable(0.0) - 0.0).abs() < 1e-6);
        assert!((tanh_stable(1.0) - 0.7615942).abs() < 1e-5, "tanh(1) ≈ 0.7615942");
        assert!((tanh_stable(-1.0) + 0.7615942).abs() < 1e-5);
        // 大 |u|：必须**饱和到 ±1 且不出 NaN/∞**
        assert_eq!(tanh_stable(1000.0), 1.0);
        assert_eq!(tanh_stable(-1000.0), -1.0);
        assert!(tanh_stable(f32::NAN).is_nan());
        assert_eq!(tanh_stable(f32::INFINITY), 1.0);
        assert_eq!(tanh_stable(f32::NEG_INFINITY), -1.0);
        // 奇函数性
        assert!((tanh_stable(2.5) + tanh_stable(-2.5)).abs() < 1e-6);
    }

    /// ★ **ρ 的符号语义**：内部（`SDF < 0`）⇒ `ρ > 0`（正源）；外部 ⇒ `ρ < 0`。
    #[test]
    fn rho_sign_semantics_and_bounds() {
        let eps = 1.5;
        assert_eq!(rho_soft_threshold(0.0, eps), 0.0, "边界上源为 0");
        assert!(rho_soft_threshold(-2.0, eps) > 0.0, "内部（SDF<0）⇒ 正源");
        assert!(rho_soft_threshold(2.0, eps) < 0.0, "外部（SDF>0）⇒ 负源");
        // 有界于 **[-1, 1]**（`|SDF| ≫ ε` 时**饱和到 ±1 是正确行为**，不是缺陷）
        for k in -50..=50 {
            let sd = k as f32 * 0.3;
            let v = rho_soft_threshold(sd, eps);
            assert!(
                (-1.0..=1.0).contains(&v),
                "ρ 必须在 [-1,1]，实测 {v}（SDF={sd}）"
            );
        }
        // 饱和：|SDF| ≥ 10ε ⇒ |ρ| 应已经贴到 1（tanh(10) ≈ 1 - 2e-9）
        assert!(rho_soft_threshold(-15.0, eps) >= 0.999_999, "深内部应饱和到 +1");
        assert!(rho_soft_threshold(15.0, eps) <= -0.999_999, "深远外部应饱和到 -1");
        // 单调性：**非增**处处成立；**严格递减只在饱和前**
        // （`|SDF| ≫ ε` 时 `tanh` 已饱和 ⇒ 相邻取值相等是**正确行为**，不是缺陷）
        let mut prev = f32::INFINITY;
        for k in -60..=60 {
            let sd = k as f32 * 0.2;
            let v = rho_soft_threshold(sd, eps);
            assert!(v <= prev, "ρ 必须非增（k={k}）");
            if sd.abs() <= 4.0 && k > -60 {
                assert!(v < prev, "饱和前（|SDF| ≤ 4）必须严格递减（k={k}）");
            }
            prev = v;
        }
        // clamp 版与 soft 版**同号**（只是形状不同）
        for k in -40..=40 {
            let s = rho_soft_threshold(k as f32 * 0.25, eps);
            let c = rho_clamp(k as f32 * 0.25, eps);
            assert!(s * c >= 0.0, "两版必须同号（k={k}）");
        }
        // ε 退化（0）不得除零/NaN
        assert_eq!(rho_soft_threshold(1.0, 0.0), -1.0);
        assert_eq!(rho_soft_threshold(-1.0, 0.0), 1.0);
        assert_eq!(rho_clamp(1.0, 0.0), -1.0);
    }

    #[test]
    fn eps_rule_is_1p5_times_grid_step() {
        assert_eq!(eps_min(1.0), 1.5);
        assert_eq!(eps_min(COARSE_GRID_STEP), 1.5);
        assert_eq!(eps_min(2.0), 3.0);
    }

    // ---------- Field2D ----------

    #[test]
    fn field2d_basics_and_safety() {
        let mut buf = field_of(5, 4, 1.0);
        {
            let mut f = Field2D::new(5, 4, &mut buf).unwrap();
            assert_eq!(f.cells(), 20);
            assert_eq!(f.idx(3, 2), 13);
            assert_eq!(f.get(3, 2), Some(1.0));
            assert_eq!(f.get(5, 0), None, "越界 ⇒ None");
            assert!(!f.set(5, 0, 9.0));
            assert!(f.set(1, 1, 7.0));
            assert_eq!(f.get(1, 1), Some(7.0));
            // Laplacian：平坦区为 0
            assert_eq!(f.laplacian_at(1, 1), Some(1.0 + 1.0 + 1.0 + 1.0 - 4.0 * 7.0));
            assert_eq!(f.laplacian_at(0, 0), None, "边框 ⇒ None");
            assert_eq!(f.laplacian_at(4, 3), None);
            // Dirichlet 边框
            let n = f.dirichlet_zero_border();
            assert_eq!(n, 2 * 5 + 2 * (4 - 2), "边框格点数 = 上下各 5 ＋ 左右各 2");
            assert_eq!(f.get(0, 0), Some(0.0));
            assert_eq!(f.get(4, 3), Some(0.0));
            assert_eq!(f.get(2, 0), Some(0.0));
            assert_eq!(f.get(2, 3), Some(0.0));
            assert_eq!(f.get(1, 1), Some(7.0), "内部不受影响");
            // 缓冲不足 ⇒ None
            let mut tiny = field_of(3, 3, 0.0);
            assert!(Field2D::new(5, 5, &mut tiny).is_none());
        }
    }

    /// **用户扰动注入**（设计稿：用户操作 ＝ 场扰动）：单点 / 高斯两种，都必须是纯算且可复现。
    #[test]
    fn perturbation_injection() {
        let mut buf = field_of(9, 9, 0.0);
        let mut f = Field2D::new(9, 9, &mut buf).unwrap();
        assert!(f.perturb(4, 4, 2.0));
        assert_eq!(f.get(4, 4), Some(2.0));
        assert!(!f.perturb(9, 0, 1.0), "越界 ⇒ false 且不写");
        // 高斯：中心最大、衰减、**确定性**
        let mut a = field_of(9, 9, 0.0);
        let mut b = field_of(9, 9, 0.0);
        let na = Field2D::new(9, 9, &mut a).unwrap().perturb_gaussian(4.0, 4.0, 1.5, 3.0);
        let nb = Field2D::new(9, 9, &mut b).unwrap().perturb_gaussian(4.0, 4.0, 1.5, 3.0);
        assert_eq!(na, 81);
        assert_eq!(nb, 81);
        assert_eq!(a, b, "同参数必须同结果");
        assert!(a[4 * 9 + 4] > a[(4 * 9 + 6)], "中心 > 偏离 2 格处");
        assert!(a[4 * 9 + 6] > a[4 * 9 + 8], "必须单调衰减");
        assert!(a[4 * 9 + 8] > 0.0, "高斯**无紧支撑** ⇒ 远端仍有微小值（不是被截断）");
    }

    // ---------- 投影 ----------

    #[test]
    fn projection_is_pure_affine_and_safe() {
        let f = [0.0f32, 0.5, 1.0, -1.0, 2.0, f32::NAN];
        let mut out = [0u8; 6];
        assert_eq!(project_u8(&f, 0.0, 1.0, &mut out), 6);
        assert_eq!(out[0], 0);
        assert_eq!(out[1], 128);
        assert_eq!(out[2], 255);
        assert_eq!(out[3], 0, "低于 lo ⇒ clamp 到 0（不 wrap）");
        assert_eq!(out[4], 255, "高于 hi ⇒ clamp 到 255");
        // 退化区间 ⇒ 全 0（不 panic、不除零）
        assert_eq!(project_u8(&f, 1.0, 1.0, &mut out), 6);
        assert!(out.iter().all(|&v| v == 0));
        // 短 out 不 panic
        let mut tiny = [0u8; 2];
        assert_eq!(project_u8(&f, 0.0, 1.0, &mut tiny), 2);
        // 灰度展开不引入色偏
        let c = gray_to_rgb24(200);
        assert_eq!((c.r, c.g, c.b), (200, 200, 200));
    }

    // ---------- 复值场 ----------

    #[test]
    fn complex_field_wave_and_superposition() {
        let (w, h) = (8usize, 8usize);
        let mut a = vec![0.0f32; w * h];
        let mut p = vec![0.0f32; w * h];
        let mut cf = ComplexField2D::new(w, h, &mut a, &mut p).unwrap();
        cf.clear();
        // 沿 x 的平面波，半波长 = 4 格 ⇒ k = 2π/8
        let k = core::f32::consts::TAU / 8.0;
        assert_eq!(cf.deposit_plane_wave(k, 0.0, 1.0), 64);
        assert!((cf.amp[0] - 1.0).abs() < 1e-6);
        // 相位必须归一在 [0, 2π)
        for v in cf.phase.iter() {
            assert!((0.0..core::f32::consts::TAU).contains(v), "相位必须归一，实测 {v}");
        }
        assert!((cf.phase[1] - k).abs() < 1e-5);
        assert!(cf.phase[4] > cf.phase[1], "相位随 x 递增（同一切片）");
        // 强度
        let mut inten = vec![0.0f32; w * h];
        assert_eq!(cf.intensity_into(&mut inten), 64);
        assert!((inten[0] - 1.0).abs() < 1e-6);
    }

    /// ★ **C19 同源判据**：`phase_difference_from_samples` 必须与 `interference` **逐位一致**。
    #[test]
    fn sample_phase_difference_is_same_source_as_interference() {
        // 造两列"有波形"的样点（不足 8 点 ⇒ interference 返回 0，也要一致）
        let mut a = vec![0.0f32; 32];
        let mut b = vec![0.0f32; 32];
        for i in 0..32 {
            a[i] = fmath::sin(core::f32::consts::TAU * i as f32 / 8.0);
            b[i] = fmath::sin(core::f32::consts::TAU * i as f32 / 8.0 + 0.7);
        }
        let mine = phase_difference_from_samples(&a, &b);
        let theirs = interference::phase_difference(&a, &b);
        assert_eq!(mine.to_bits(), theirs.to_bits(), "★ 必须逐位一致（同源，不是两份实现）");
        // 短输入（<8 点）⇒ 两侧都 0，仍须一致
        assert_eq!(phase_difference_from_samples(&a[..4], &b[..4]).to_bits(), 0.0f32.to_bits());
        // 显式相位差是**另一个概念**：直接相减，不做圆环归一（不是同一个判据）
        let ta = [0.1f32, 5.0];
        let tb = [0.4f32, 0.5];
        assert!((phase_difference_explicit(&ta, &tb, 0).unwrap() + 0.3).abs() < 1e-6);
        assert!((phase_difference_explicit(&ta, &tb, 1).unwrap() - 4.5).abs() < 1e-6, "可为负、不归一");
        assert_eq!(phase_difference_explicit(&ta, &tb, 2), None, "越界 ⇒ None");
    }

    #[test]
    fn superpose_is_physics_correct_on_canonical_cases() {
        // 同相 ⇒ |Ψ| = A + B
        let a = [1.0f32];
        let pa = [0.0f32];
        let b = [2.0f32];
        let pb = [0.0f32];
        let mut oa = [0.0f32];
        let mut op = [0.0f32];
        assert_eq!(superpose(&a, &pa, &b, &pb, &mut oa, &mut op), 1);
        assert!((oa[0] - 3.0).abs() < 1e-5, "同相 ⇒ 相长（3）");
        assert!(fmath::abs_f32(op[0]) < 1e-5);
        // 反相 ⇒ |Ψ| = |A − B| = 1，相位 π
        let pb2 = [core::f32::consts::PI];
        superpose(&a, &pa, &b, &pb2, &mut oa, &mut op);
        assert!((oa[0] - 1.0).abs() < 1e-4, "反相 ⇒ 相消（1），实测 {}", oa[0]);
        assert!((op[0] - core::f32::consts::PI).abs() < 1e-3, "反相相位应为 π，实测 {}", op[0]);
        // 相位差 π/2 且 A=B=1 ⇒ |Ψ| = √2
        let b1 = [1.0f32];
        let ph = [core::f32::consts::FRAC_PI_2];
        superpose(&a, &pa, &b1, &ph, &mut oa, &mut op);
        assert!((oa[0] - core::f32::consts::SQRT_2).abs() < 1e-4, "正交 ⇒ √2，实测 {}", oa[0]);
    }

    // ---------- 节律（import engine_select） ----------

    /// ★ **节律只返回数、不改场**；且 `steps ≥ 1`（永不 0）。
    #[test]
    fn rhythm_steps_is_import_based_and_never_zero() {
        let pools = [
            EnergyPool::new(),
            EnergyPool { flow_in: 100.0, flow_out: 0.0, stored: 1000.0 },
            EnergyPool { flow_in: 0.0, flow_out: 100.0, stored: 0.0 },
            EnergyPool { flow_in: 5.0, flow_out: 5.0, stored: 10.0 },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for p in &pools {
            for seed in [0.0f32, 0.25, 0.5, 0.9, 1.0] {
                let (e, n) = rhythm_steps(p, seed, 8);
                assert!(n >= 1, "steps 必须 ≥1（否则「不推进」会让判据假绿）");
                assert!(n <= 9, "steps 必须 ≤ base+1，实测 {n}");
                seen.insert(format!("{e:?}"));
                // 与 engine_select 的选择**同源**（同一个 EnergyPool ⇒ 同一个引擎）
                assert_eq!(e, engine_select::select_engine(p), "引擎选择必须来自 engine_select（同源）");
            }
        }
        assert!(seen.len() >= 2, "不同能量储备应能选出不同引擎，实测只出现 {seen:?}");
    }

    // ---------- 端到端（SDF → ρ → 泊松 → 投影） ----------

    /// ★ **端到端**：`SDF` → `ρ`（软阈值）→ 多重网格解 `∇²V = ρ` → **投影成像素**。
    ///
    /// 判据（全部可复算）：
    /// ① 解满足离散方程（残差达 `f32` 地板）；
    /// ② 画面**非平凡**（有最暗、最亮、中间灰阶）；
    /// ③ 边框最暗（左端 `lo`；Dirichlet `V = 0` ⇒ 投影为 0）；
    /// ④ **几何可见**：圆内（`ρ > 0`）的势**低于**远角（`ρ < 0`）的势，且差异是量程的可观比例；
    /// ⑤ **确定性**（跑两次逐字节相同）。
    ///
    /// ⚠️ **两处期望值自纠（写在这里以免后人重踩）**：
    /// - 初版写「圆心最亮」⇒ **实测为 0**。根因：本题 `∫ρ dA < 0`（背景面积远大于圆），
    ///   Dirichlet 格林函数 `G < 0` ⇒ `V = ∫Gρ` **在背景处也被抬高**；
    /// - 更本质的一条：`ρ = −tanh(SDF/ε)`（**已裁定的公式**）在内部取正值，
    ///   `∇²V = ρ > 0` 意味着 `V` 在内部**下凹** ⇒ **圆心是最暗处**，不是最亮处。
    ///   ⇒ 判据改为「**圆内势 < 远角势**」这一**由机制必然导出**的事实（不再猜最亮点位置）。
    #[test]
    fn end_to_end_sdf_to_pixels() {
        // ★ 网格须为 `2^k + 1`（多重网格的边界对齐要求）
        let (w, h) = (33usize, 33usize);
        let mut sc_buf = vec![0.0f32; poisson::scratch_len_auto(w, h, 4).unwrap()];

        // 返回 (像素, V_max, V_min, V(圆心), V(远角))
        let run = |sc: &mut [f32]| -> (Vec<u8>, f32, f32, f32, f32) {
            let mut sdf_buf = vec![0.0f32; w * h];
            let n = sdf::sample_into(|x, y| sdf::circle(16.0, 16.0, 8.0, x, y), w, h, &mut sdf_buf);
            assert_eq!(n, w * h);
            let eps = eps_min(COARSE_GRID_STEP);
            let mut rho = vec![0.0f32; w * h];
            for i in 0..w * h {
                rho[i] = rho_soft_threshold(sdf_buf[i], eps);
            }
            let mut phi = vec![0.0f32; w * h];
            let opt = poisson::MgOptions { max_cycles: 60, ..poisson::MgOptions::f32_default() };
            let rep = poisson::solve_poisson_mg(&rho, &mut phi, w, h, sc, &opt).unwrap();
            assert!(
                rep.converged,
                "端到端必须收敛：r={} gate={} floor={}",
                rep.residual_inf, rep.gate, rep.roundoff_floor
            );
            let mut vmax = f32::NEG_INFINITY;
            let mut vmin = f32::INFINITY;
            for v in &phi {
                if *v > vmax {
                    vmax = *v;
                }
                if *v < vmin {
                    vmin = *v;
                }
            }
            assert!(vmax > vmin, "势必须有起伏（否则投影无意义）");
            // ③ Dirichlet 必须**逐点精确成立**（不是"看起来暗"）——这是 f32 层面的硬判据
            for k in 0..w {
                assert_eq!(phi[k], 0.0, "上边框必须精确 V=0（k={k}）");
                assert_eq!(phi[(h - 1) * w + k], 0.0, "下边框必须精确 V=0");
            }
            for k in 0..h {
                assert_eq!(phi[k * w], 0.0, "左边框必须精确 V=0（k={k}）");
                assert_eq!(phi[k * w + w - 1], 0.0, "右边框必须精确 V=0");
            }
            let mut px = vec![0u8; w * h];
            assert_eq!(project_u8(&phi, vmin, vmax, &mut px), w * h);
            let (vc, vk) = (phi[16 * w + 16], phi[1 * w + 1]);
            (px, vmax, vmin, vc, vk)
        };

        let (a, vmax, vmin, vc, vk) = run(&mut sc_buf);
        let (b, ..) = run(&mut sc_buf);
        assert_eq!(a, b, "⑤ 必须确定性（逐字节相同）");

        println!(
            "[field·e2e] V∈[{vmin:.4},{vmax:.4}] 圆心={vc:.4} 远角={vk:.4} 量程={:.4}",
            vmax - vmin
        );

        // ② 非平凡（用**全量程**投影：vmin→0、vmax→255）
        let mn = *a.iter().min().unwrap();
        let mx = *a.iter().max().unwrap();
        assert_eq!(mx, 255, "应出现最亮（全量程投影 ⇒ 必为 255）");
        assert_eq!(mn, 0, "应出现最暗（全量程投影 ⇒ 必为 0）");
        let n_mid = a.iter().filter(|&&v| v > 20 && v < 235).count();
        assert!(n_mid > 50, "应有中间灰阶（说明是渐变场，不是二值跳变），实测 {n_mid}");

        // ③ 已在上方以「`phi` 边框精确 == 0」判定（**比"看起来暗"强得多**）
        // ④ 几何可见：圆内势 < 远角势，且差异 ≥ 量程的 5%
        let range = vmax - vmin;
        assert!(vc < vk, "圆内势应低于远角势（ρ 内部为正 ⇒ V 下凹）：圆心={vc} 远角={vk}");
        assert!(
            (vk - vc) >= 0.05 * range,
            "圆内与远角的势差 ({:.4}) 应 ≥ 量程的 5% ({:.4})",
            vk - vc,
            0.05 * range
        );
    }

}
