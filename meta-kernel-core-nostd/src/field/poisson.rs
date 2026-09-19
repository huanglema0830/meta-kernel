//! # 场方程求解器 · **多重网格泊松求解**（纯算层子模块）
//!
//! 来源：`coordination/discussions/2026-09-19_场方程求解器设计稿.md`
//! §2.2.2(1)（主方程 `∇²V = ρ`）、§2.2.3(2)（多重网格）、§2.2.4（纯算层 §1）。
//!
//! ## ★ 为什么多重网格是「可行性项」而不是「优化项」
//!
//! | 方法 | 迭代次数（`n×n` Poisson） | 总复杂度 |
//! |---|---|---|
//! | Jacobi / Gauss-Seidel（单层） | `O(n²)` | **`O(n⁴)`** ⇒ 256×256 上不可行 |
//! | CG（无预条件） | `O(n)` | `O(n³)` |
//! | **多重网格（V-cycle）** | **`O(1)`** | **`O(N)`** |
//!
//! ⇒ 本模块是本轮「能算得动」的前提，**不是「以后优化」的备选**。
//!
//! ## 数学口径（写死，勿猜）
//!
//! | 项 | 约定 |
//! |---|---|
//! | **离散算子** | 五点差分：`(Σ邻居 − 4V)/h² = ρ` |
//! | **网格间距** | 最细层 `h₀ = 1`；第 `l` 层 `h_l = 2^l` ⇒ `h_l² = 4^l`（**无量纲**，「格距 = 1」） |
//! | **边界条件** | **Dirichlet `V = 0` 在边框**（设计稿 §2.2.2(4) 的倾向）：边框点**不迭代**、其残差**定义为 0** |
//! | **松弛** | **红黑 Gauss-Seidel**（可原地、无数据依赖链、天然可向量化） |
//! | **限制（细→粗）** | **残差限制**（**不是解限制**），全权重 `[1 2 1; 2 4 2; 1 2 1]/16` |
//! | **插值（粗→细）** | 双线性（**加性**修正） |
//! | **收敛判据** | `max|r| ≤ tol · max(1, max|ρ|)`（**判据可复现**：无随机源） |
//! | **内存** | **全部静态切片**（调用方给 `scratch`）；**不用 `Vec`**（单帧 4 KiB 上限 ⇒ 由调用方控量） |
//!
//! ## `scratch` 布局（**唯一权威在此**；[`scratch_len`] 与 [`solve_poisson_mg`] 按同一定义实现）
//!
//! ```text
//!  [ res        ] len₀                      ← 残差临时区（各级**共用**：算完立刻限制走）
//!  [ rhs₀ phi₀ ] len₀ len₀                  ← 第 0 层（rhs₀ 由调用方 rho 拷入，保持各级代码统一）
//!  [ rhs₁ phi₁ ] len₁ len₁
//!  ...
//!  [ rhs_{L-1} phi_{L-1} ]
//! ```
//! ⇒ `need = len₀ + 2 · Σ_{l=0}^{L-1} len_l`。
//!
//! ## ★ 网格尺寸必须是 `2^k + 1`（**边界对齐铁律**）
//!
//! 例：`33 → 17 → 9 → 5`（`w_l = ((w − 1) >> l) + 1`）。
//!
//! **为什么**：粗层第 `j` 点必须对应细层第 `2j` 点，且**两端边界点都要对齐**
//! （`j = cw−1 ↔ 2(cw−1) = fw−1`）。若写成 `w >> l`（`fw = 2·cw`），细层**最后一行/列没有粗层对应点**，
//! 粗层会把自己那端的 Dirichlet 强加到**错误的物理位置**。
//!
//! **实测代价（本轮踩坑记录）**：`32` 点时 1 个 V-cycle 的误差只降 71%、且**层数越多越差**；
//! 改成 `33` 点后同一算式变成 **每 cycle 降 8～60 倍**。⇒ 尺寸不合法一律返回 [`MgError::BadDims`]，
//! **绝不静默丢层**。
//!
//! ## 已知边界（**不掩盖**）
//!
//! - **只解线性系统**：若 `ρ` 依赖 `V`（非线性），须**外层不动点迭代**（V-cycle 当内层）
//!   ⇒ **复杂度分析要重做**（设计稿 §2.2.3(2) 的口径）。本模块**不**做外层迭代。
//! - **要求各层尺寸整除**（`w`、`h` 都能被 `2^(levels-1)` 整除）；不满足 ⇒ [`MgError::BadDims`]。

use crate::fmath;

/// 允许的最大层数（**编译期定长**，避免 `Vec`）。
pub const MAX_LEVELS: usize = 16;

/// 求解器错误（**全部可判、无 `panic`**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MgError {
    /// `w`／`h` 太小，或**层数下尺寸不能整除**
    BadDims,
    /// `levels` 超出 [`MAX_LEVELS`] 或为 0
    TooManyLevels,
    /// `scratch` 不足（给出所需长度）
    ScratchTooSmall {
        /// 需要的 `f32` 个数
        need: usize,
    },
}

/// 求解参数。
#[derive(Clone, Copy, Debug)]
pub struct MgOptions {
    /// 层数；`0` ＝ **自动**（取到最粗层最小边 ≥ 4 为止）
    pub levels: usize,
    /// 下行前松弛次数 `ν₁`
    pub nu1: u32,
    /// 上行后松弛次数 `ν₂`
    pub nu2: u32,
    /// 最粗层直接松弛次数
    pub coarsest_sweeps: u32,
    /// 最大 V-cycle 次数
    pub max_cycles: u32,
    /// 相对收敛门线（`max|r| ≤ tol · max(1, max|ρ|)`）；`0.0` ⇒ **只跑固定 cycle 数**
    pub tol: f32,
}

impl Default for MgOptions {
    fn default() -> Self {
        Self { levels: 0, nu1: 2, nu2: 2, coarsest_sweeps: 40, max_cycles: 60, tol: 1.0e-6 }
    }
}

impl MgOptions {
    /// **`f32` 工况的默认参数**：`tol` 取在"**略高于 `f32` 舍入地板**"的量级。
    ///
    /// ★ **这是本项目一条必须写死的口径**（设计稿 §2.2.3(3) 的警示）：
    /// `f32` 的残差**存在地板**（见 [`roundoff_floor_f32`]），
    /// **直接照搬 `f64` 的 `tol = 1e-9` 会永远不收敛**（本轮实测：`32×32` 上残差停在 `3.4e-7`）。
    #[must_use]
    pub fn f32_default() -> Self {
        Self { tol: 32.0 * f32::EPSILON, ..Default::default() }
    }
}

/// **`f32` 残差地板**：五点 Laplacian 的**舍入噪声**量级 ⇒ **收敛门线不得显著低于它**。
///
/// **推导 ＋ 实测支撑（两者都要有）**：
/// - **推导**：残差 `r = ρ − (Σ邻 − 4V)/h²` 含 **5 个 `O(max|V|)` 的加减**，
///   每步相对舍入 ≤ `ε_f32/2` ⇒ 绝对噪声 ≈ `k·ε_f32·max|V|`；
/// - **实测（2026-09-19，`33×33` 制造解）**：`max|V| ≈ 1.0008` 时残差停在 **`2.83e-6`**
///   ⇒ `k ≈ 2.83e-6 / ε_f32 ≈ 24`。**取 `k = 32`** 作保守估计（留 ~33% 余量）。
/// - `max|V| < 1` 时按 1 计（避免地板过小）。
///
/// ⇒ **判据口径**：`max|r|` 降到该地板同量级即为「`f32` 下已解到位」；
/// **再往下压不是「更准」，只是在压舍入噪声**。
#[must_use]
pub fn roundoff_floor_f32(v_inf: f32) -> f32 {
    let v = if v_inf > 1.0 { v_inf } else { 1.0 };
    32.0 * f32::EPSILON * v
}

/// 求解报告（**报数写口径**：每个数都能被独立复算）。
#[derive(Clone, Copy, Debug)]
pub struct MgReport {
    /// 实际使用的层数
    pub levels: usize,
    /// 实际跑了几次 V-cycle
    pub cycles: u32,
    /// 结束时的 `max|r|`（最细层）
    pub residual_inf: f32,
    /// **第一次** V-cycle 之后的 `max|r|`（`0` ＝ 尚未跑过）
    pub residual_after_first: f32,
    /// `max|ρ|`（收敛门线的分母）
    pub rho_inf: f32,
    /// 结束时解的 `max|V|`（用于算 f32 舍入地板）
    pub v_inf: f32,
    /// `f32` 舍入地板（见 [`roundoff_floor_f32`]）—— **残差不可能有意义地低于它**
    pub roundoff_floor: f32,
    /// 最终使用的收敛门线 `tol · max(1, max|ρ|, max|V|)`
    pub gate: f32,
    /// 是否达到门线
    pub converged: bool,
}

impl MgReport {
    /// 自第一次 V-cycle 起的**残差下降倍数**；不可算 ⇒ `None`。
    ///
    /// ★ 这是「多重网格有效」的直接证据：单层 GS 在同样 cycle 数下**远达不到**该倍数。
    #[must_use]
    pub fn reduction_per_cycle(&self) -> Option<f32> {
        if self.cycles == 0 || self.residual_after_first <= 0.0 || self.residual_inf <= 0.0 {
            return None;
        }
        Some(self.residual_after_first / self.residual_inf)
    }
}

#[inline]
fn idx(x: usize, y: usize, w: usize) -> usize {
    y * w + x
}

/// 第 `l` 层的尺寸。**★ 网格尺寸必须是 `2^k + 1`**（见模块头「边界对齐」）。
///
/// 定义：`w_l = ((w − 1) >> l) + 1` —— 即"**去掉一端后折半，再补回端点**"。
/// ⇒ 粗层第 `j` 点 ↔ 细层第 `2j` 点，**两端边界点都对齐**（`j = cw−1 ↔ 2(cw−1) = fw−1`）。
///
/// ⚠️ **为什么不能用 `w >> l`**（我踩过）：那样 `fw = 2·cw`，细层**最后一行/列没有粗层对应点**，
/// 粗层会把自己那端的 Dirichlet 强加到**错误的物理位置** ⇒ 粗层修正量系统性偏小
/// （实测：33 点改 32 点后，1 个 V-cycle 的误差只降 71%，且层数越多越差）。
#[must_use]
pub const fn level_dims(w: usize, h: usize, l: usize) -> (usize, usize) {
    (((w - 1) >> l) + 1, ((h - 1) >> l) + 1)
}

/// 自动层数：**自大而小**直到最粗层的最小边 **< `min_dim`**（建议 `min_dim = 5`，即最粗 5×5）。
#[must_use]
pub fn auto_levels(w: usize, h: usize, min_dim: usize) -> usize {
    let mut l = 1usize;
    while l < MAX_LEVELS {
        let (cw, ch) = level_dims(w, h, l);
        if cw.min(ch) < min_dim {
            break;
        }
        l += 1;
    }
    l
}

/// `scratch` 所需长度（`f32` 个数）；溢出／非法 ⇒ `None`。
#[must_use]
pub fn scratch_len(w: usize, h: usize, levels: usize) -> Option<usize> {
    if levels == 0 || levels > MAX_LEVELS {
        return None;
    }
    let mut total = w.checked_mul(h)?; // res 区（len₀）
    for l in 0..levels {
        let (cw, ch) = level_dims(w, h, l);
        let len = cw.checked_mul(ch)?;
        total = total.checked_add(len.checked_mul(2)?)?;
    }
    Some(total)
}

/// **自动层数下的 `scratch` 需求**（最常用入口：调用方不必自己算层数）。
#[must_use]
pub fn scratch_len_auto(w: usize, h: usize, min_dim: usize) -> Option<usize> {
    scratch_len(w, h, auto_levels(w, h, min_dim))
}

/// `max|·|`：**NaN 一旦出现即整体判 NaN**（⇒ 不会静默通过收敛门线）。
fn max_abs(v: &[f32]) -> f32 {
    let mut m = 0.0f32;
    let mut has_nan = false;
    for &x in v {
        let a = fmath::abs_f32(x);
        if a.is_nan() {
            has_nan = true;
            continue;
        }
        if a > m {
            m = a;
        }
    }
    if has_nan { f32::NAN } else { m }
}

/// 内部点的 `max|ρ|`（**判据的单一实现**：收敛门线与外部核对都用它，C19 同源）。
pub fn rho_inf(rho: &[f32], w: usize, h: usize) -> f32 {
    if w < 3 || h < 3 {
        return max_abs(rho);
    }
    let mut m = 0.0f32;
    let mut y = 1usize;
    while y + 1 < h {
        let mut x = 1usize;
        while x + 1 < w {
            let a = fmath::abs_f32(rho[idx(x, y, w)]);
            if a > m {
                m = a;
            }
            x += 1;
        }
        y += 1;
    }
    m
}

/// **红黑 Gauss-Seidel 松弛**（原地；边框点不迭代）。`h2` ＝ `h_l²`。
pub fn gs_sweeps(phi: &mut [f32], rho: &[f32], w: usize, h: usize, h2: f32, sweeps: u32) {
    if w < 3 || h < 3 {
        return;
    }
    let mut s = 0u32;
    while s < sweeps {
        let mut color = 0usize;
        while color < 2 {
            let mut y = 1usize;
            while y + 1 < h {
                // 红黑交替：同色内无数据依赖 ⇒ 可原地、可向量化
                let mut x = 1 + ((color + y) & 1);
                while x + 1 < w {
                    let i = idx(x, y, w);
                    let nb = phi[i - 1] + phi[i + 1] + phi[i - w] + phi[i + w];
                    phi[i] = 0.25 * (nb - h2 * rho[i]);
                    x += 2;
                }
                y += 1;
            }
            color += 1;
        }
        s += 1;
    }
}

/// 残差的 `max|r|`（边框点定义为 0，见文件头口径）。
pub fn residual_inf(phi: &[f32], rho: &[f32], w: usize, h: usize, h2: f32) -> f32 {
    if w < 3 || h < 3 {
        return 0.0;
    }
    let mut m = 0.0f32;
    let mut y = 1usize;
    while y + 1 < h {
        let mut x = 1usize;
        while x + 1 < w {
            let i = idx(x, y, w);
            let nb = phi[i - 1] + phi[i + 1] + phi[i - w] + phi[i + w];
            let r = rho[i] - (nb - 4.0 * phi[i]) / h2;
            let a = fmath::abs_f32(r);
            if a > m {
                m = a;
            }
            x += 1;
        }
        y += 1;
    }
    m
}

/// `h_l² = 4^l`（最细层 `h₀ = 1`）。
#[inline]
fn pow4(l: usize) -> f32 {
    let mut v = 1.0f32;
    let mut i = 0;
    while i < l {
        v *= 4.0;
        i += 1;
    }
    v
}

fn s_copy(dst: &mut [f32], src: &[f32]) {
    let n = dst.len().min(src.len());
    let mut i = 0;
    while i < n {
        dst[i] = src[i];
        i += 1;
    }
}

// ---------- scratch 内操作（统一用「一块 &mut ＋ 偏移」，避免同时借用两个区域） ----------

fn gs_in_scratch(s: &mut [f32], rho_off: usize, phi_off: usize, w: usize, h: usize, h2: f32, sweeps: u32) {
    if w < 3 || h < 3 || sweeps == 0 {
        return;
    }
    let mut k = 0u32;
    while k < sweeps {
        let mut color = 0usize;
        while color < 2 {
            let mut y = 1usize;
            while y + 1 < h {
                let mut x = 1 + ((color + y) & 1);
                while x + 1 < w {
                    let i = idx(x, y, w);
                    let nb = s[phi_off + i - 1]
                        + s[phi_off + i + 1]
                        + s[phi_off + i - w]
                        + s[phi_off + i + w];
                    s[phi_off + i] = 0.25 * (nb - h2 * s[rho_off + i]);
                    x += 2;
                }
                y += 1;
            }
            color += 1;
        }
        k += 1;
    }
}

/// `res = ρ − (Σ邻 − 4V)/h²`（边框 0）；返回 `max|r|`。
fn res_in_scratch(
    s: &mut [f32],
    rho_off: usize,
    phi_off: usize,
    res_off: usize,
    w: usize,
    h: usize,
    h2: f32,
) -> f32 {
    let len = w * h;
    for v in s[res_off..res_off + len].iter_mut() {
        *v = 0.0;
    }
    if w < 3 || h < 3 {
        return 0.0;
    }
    let mut m = 0.0f32;
    let mut y = 1usize;
    while y + 1 < h {
        let mut x = 1usize;
        while x + 1 < w {
            let i = idx(x, y, w);
            let nb = s[phi_off + i - 1] + s[phi_off + i + 1] + s[phi_off + i - w] + s[phi_off + i + w];
            let r = s[rho_off + i] - (nb - 4.0 * s[phi_off + i]) / h2;
            s[res_off + i] = r;
            let a = fmath::abs_f32(r);
            if a > m {
                m = a;
            }
            x += 1;
        }
        y += 1;
    }
    m
}

/// 残差限制（细 → 粗，全权重 `[1 2 1; 2 4 2; 1 2 1]/16`）；细侧越界取 0（Dirichlet）。
fn restrict_in_scratch(
    s: &mut [f32],
    res_off: usize,
    fw: usize,
    fh: usize,
    rhs_off: usize,
    cw: usize,
    ch: usize,
) {
    let mut y = 0usize;
    while y < ch {
        let mut x = 0usize;
        while x < cw {
            let fx = (2 * x) as isize;
            let fy = (2 * y) as isize;
            let mut v = 4.0 * fine_at(s, res_off, fw, fh, fx, fy);
            v += 2.0
                * (fine_at(s, res_off, fw, fh, fx - 1, fy)
                    + fine_at(s, res_off, fw, fh, fx + 1, fy)
                    + fine_at(s, res_off, fw, fh, fx, fy - 1)
                    + fine_at(s, res_off, fw, fh, fx, fy + 1));
            v += fine_at(s, res_off, fw, fh, fx - 1, fy - 1)
                + fine_at(s, res_off, fw, fh, fx + 1, fy - 1)
                + fine_at(s, res_off, fw, fh, fx - 1, fy + 1)
                + fine_at(s, res_off, fw, fh, fx + 1, fy + 1);
            s[rhs_off + idx(x, y, cw)] = v / 16.0;
            x += 1;
        }
        y += 1;
    }
}

#[inline]
fn fine_at(s: &[f32], off: usize, w: usize, h: usize, x: isize, y: isize) -> f32 {
    if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
        return 0.0;
    }
    s[off + idx(x as usize, y as usize, w)]
}

/// 双线性插值（粗 → 细）**加性**写入。
fn prolong_add_in_scratch(
    s: &mut [f32],
    coarse_off: usize,
    cw: usize,
    ch: usize,
    fine_off: usize,
    fw: usize,
    fh: usize,
) {
    let mut y = 0usize;
    while y < fh {
        let mut x = 0usize;
        while x < fw {
            let cx = (x / 2) as isize;
            let cy = (y / 2) as isize;
            let rx = (x % 2) as f32;
            let ry = (y % 2) as f32;
            let a = fine_at(s, coarse_off, cw, ch, cx, cy);
            let b = fine_at(s, coarse_off, cw, ch, cx + 1, cy);
            let c = fine_at(s, coarse_off, cw, ch, cx, cy + 1);
            let d = fine_at(s, coarse_off, cw, ch, cx + 1, cy + 1);
            let top = a * (1.0 - rx) + b * rx;
            let bot = c * (1.0 - rx) + d * rx;
            s[fine_off + idx(x, y, fw)] += top * (1.0 - ry) + bot * ry;
            x += 1;
        }
        y += 1;
    }
}

/// **多重网格 V-cycle 求解 `∇²V = ρ`**（Dirichlet `V=0` 边框）。
///
/// - `rho`：源项（长度 `w*h`，行优先）
/// - `phi`：**初值 ＋ 输出**（长度 `w*h`；**原地**）
/// - `scratch`：工作区，长度须 ≥ [`scratch_len`]
/// - **越界/非法一律返回 `Err`，不 `panic`**
pub fn solve_poisson_mg(
    rho: &[f32],
    phi: &mut [f32],
    w: usize,
    h: usize,
    scratch: &mut [f32],
    opt: &MgOptions,
) -> Result<MgReport, MgError> {
    // ★ 网格尺寸必须是 `2^k + 1`（含两端边界点）；否则粗层边界点无法对齐（见 `level_dims`）
    if w < 5 || h < 5 || rho.len() < w * h || phi.len() < w * h {
        return Err(MgError::BadDims);
    }
    let levels = if opt.levels == 0 { auto_levels(w, h, 4) } else { opt.levels };
    if levels == 0 || levels > MAX_LEVELS {
        return Err(MgError::TooManyLevels);
    }
    // 各层尺寸必须整除（否则 `>>` 静默丢行）
    for l in 0..levels {
        let (cw, ch) = level_dims(w, h, l);
        if cw < 3 || ch < 3 || ((cw - 1) << l) != w - 1 || ((ch - 1) << l) != h - 1 {
            return Err(MgError::BadDims);
        }
    }
    let len0 = w * h;
    let need = match scratch_len(w, h, levels) {
        Some(v) => v,
        None => return Err(MgError::ScratchTooSmall { need: usize::MAX }),
    };
    if scratch.len() < need {
        return Err(MgError::ScratchTooSmall { need });
    }

    // ---- 布局（与 `scratch_len` 严格同源：一处改、另一处必须跟着改）----
    let res_off = 0usize;
    let mut offs = [(0usize, 0usize); MAX_LEVELS]; // (rhs_off, phi_off)
    let mut cur = len0;
    for (l, o) in offs.iter_mut().enumerate().take(levels) {
        let (cw, ch) = level_dims(w, h, l);
        let len = cw * ch;
        *o = (cur, cur + len);
        cur += 2 * len;
    }

    let rho_abs = rho_inf(rho, w, h); // 判据单一实现（C19 同源）

    s_copy(&mut scratch[offs[0].0..offs[0].0 + len0], rho);
    s_copy(&mut scratch[offs[0].1..offs[0].1 + len0], phi);

    let mut cycles = 0u32;
    let mut after_first = 0.0f32;
    let mut last = f32::NAN;
    let mut gate = opt.tol; // 门线（每 cycle 按问题量级重算）
    while cycles < opt.max_cycles {
        // ---- 下行 ----
        for l in 0..levels - 1 {
            let (cw, ch) = level_dims(w, h, l);
            let (rho_o, phi_o) = offs[l];
            gs_in_scratch(scratch, rho_o, phi_o, cw, ch, pow4(l), opt.nu1);
            res_in_scratch(scratch, rho_o, phi_o, res_off, cw, ch, pow4(l));
            let (ncw, nch) = level_dims(w, h, l + 1);
            let (nrho_o, nphi_o) = offs[l + 1];
            restrict_in_scratch(scratch, res_off, cw, ch, nrho_o, ncw, nch);
            // 粗层解从 0 起（V-cycle 解的是**误差方程**）
            for v in scratch[nphi_o..nphi_o + ncw * nch].iter_mut() {
                *v = 0.0;
            }
        }
        // ---- 最粗层直解 ----
        #[cfg(test)]
        if opt.max_cycles == 777 {
            for l in 0..levels {
                let (cw, ch) = level_dims(w, h, l);
                let (rho_o, phi_o) = offs[l];
                println!(
                    "[dbg] after descend level={l} dims={cw}x{ch} max|rhs|={:.4e} max|phi|={:.4e}",
                    max_abs(&scratch[rho_o..rho_o + cw * ch]),
                    max_abs(&scratch[phi_o..phi_o + cw * ch])
                );
            }
        }
        {
            let l = levels - 1;
            let (cw, ch) = level_dims(w, h, l);
            let (rho_o, phi_o) = offs[l];
            gs_in_scratch(scratch, rho_o, phi_o, cw, ch, pow4(l), opt.coarsest_sweeps);
        }
        // ---- 上行 ----
        let mut l = levels - 1;
        while l > 0 {
            let (cw, ch) = level_dims(w, h, l);
            let (fw, fh) = level_dims(w, h, l - 1);
            let coarse_phi_o = offs[l].1;
            let (rho_o, phi_o) = offs[l - 1];
            prolong_add_in_scratch(scratch, coarse_phi_o, cw, ch, phi_o, fw, fh);
            gs_in_scratch(scratch, rho_o, phi_o, fw, fh, pow4(l - 1), opt.nu2);
            l -= 1;
        }
        #[cfg(test)]
        if opt.max_cycles == 777 {
            for l in 0..levels {
                let (cw, ch) = level_dims(w, h, l);
                let (rho_o, phi_o) = offs[l];
                println!(
                    "[dbg] after ascend level={l} dims={cw}x{ch} max|rhs|={:.4e} max|phi|={:.4e}",
                    max_abs(&scratch[rho_o..rho_o + cw * ch]),
                    max_abs(&scratch[phi_o..phi_o + cw * ch])
                );
            }
        }
        cycles += 1;
        let (rho_o, phi_o) = offs[0];
        last = res_in_scratch(scratch, rho_o, phi_o, res_off, w, h, 1.0);
        if cycles == 1 {
            after_first = last;
        }
        // ★ 门线**按问题量级缩放**（`f32` 地板 ∝ max|V|；只按 `max|ρ|` 定会在 `max|V|≫1` 时永不到线）
        let v_now = max_abs(&scratch[phi_o..phi_o + len0]);
        let scale = if v_now > rho_abs { v_now } else { rho_abs };
        let scale = if scale > 1.0 { scale } else { 1.0 };
        gate = opt.tol * scale;
        if last <= gate {
            break;
        }
    }

    // 结果写回调用方
    let phi0 = offs[0].1;
    let mut v_inf = 0.0f32;
    for i in 0..len0 {
        let v = scratch[phi0 + i];
        phi[i] = v;
        let a = fmath::abs_f32(v);
        if a > v_inf {
            v_inf = a;
        }
    }
    Ok(MgReport {
        levels,
        cycles,
        residual_inf: last,
        residual_after_first: after_first,
        rho_inf: rho_abs,
        v_inf,
        roundoff_floor: roundoff_floor_f32(v_inf),
        gate,
        converged: last <= gate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_of(w: usize, h: usize, levels: usize) -> Vec<f32> {
        // `levels == 0` ⇒ 走**自动层数**（与 `solve_poisson_mg` 的默认口径一致）
        let lv = if levels == 0 { auto_levels(w, h, 4) } else { levels };
        vec![0.0; scratch_len(w, h, lv).unwrap()]
    }

    #[test]
    fn layout_and_errors() {
        // ★ 网格尺寸须为 `2^k + 1` ⇒ 33 → 17 → 9 → 5
        assert_eq!(level_dims(33, 33, 0), (33, 33));
        assert_eq!(level_dims(33, 33, 1), (17, 17));
        assert_eq!(level_dims(33, 33, 2), (9, 9));
        assert_eq!(level_dims(33, 33, 3), (5, 5));
        assert_eq!(auto_levels(33, 33, 4), 4, "33→17→9→5（再粗到 3 就 < min_dim=4）");
        assert_eq!(auto_levels(33, 33, 5), 4);

        // `scratch_len` 必须与 `level_dims` **同源**（此处按定义复算，而不是抄一个魔数）
        let want = |w: usize, h: usize, lv: usize| -> usize {
            let mut t = w * h;
            for l in 0..lv {
                let (cw, ch) = level_dims(w, h, l);
                t += 2 * cw * ch;
            }
            t
        };
        assert_eq!(scratch_len(33, 33, 4), Some(want(33, 33, 4)));
        assert_eq!(scratch_len(17, 17, 3), Some(want(17, 17, 3)));
        assert_eq!(scratch_len(33, 33, 0), None, "层数 0 对 scratch_len 非法（须先定层数）");
        assert_eq!(scratch_len(33, 33, MAX_LEVELS + 1), None);

        // 错误路径（不 panic）
        let (w, h) = (33usize, 33usize);
        let rho = vec![0.0f32; w * h];
        let mut phi = vec![0.0f32; w * h];
        let mut small = vec![0.0f32; 4];
        assert!(matches!(
            solve_poisson_mg(&rho, &mut phi, w, h, &mut small, &MgOptions::default()),
            Err(MgError::ScratchTooSmall { need }) if need == want(w, h, 4)
        ));
        assert!(matches!(
            solve_poisson_mg(&rho, &mut phi, 3, 3, &mut small, &MgOptions::default()),
            Err(MgError::BadDims)
        ));
        // ★ 非 `2^k + 1` 必须 BadDims（**而不是静默丢层**）：30 点 ⇒ (29−1) 不能整除 2 的幂
        let mut s2 = vec![0.0f32; 65536];
        let mut p2 = vec![0.0f32; 30 * 30];
        let r2 = vec![0.0f32; 30 * 30];
        let opt2 = MgOptions { levels: 2, ..Default::default() };
        assert!(matches!(
            solve_poisson_mg(&r2, &mut p2, 30, 30, &mut s2, &opt2),
            Err(MgError::BadDims)
        ));
    }

    /// 齐次问题（`ρ = 0`、边框 `V=0`）的唯一解是 `V ≡ 0`；从非零初值出发必须收敛到 0。
    #[test]
    fn homogeneous_decays_to_zero() {
        let (w, h) = (17usize, 17usize);
        let rho = vec![0.0f32; w * h];
        let mut phi = vec![1.0f32; w * h];
        for x in 0..w {
            phi[idx(x, 0, w)] = 0.0;
            phi[idx(x, h - 1, w)] = 0.0;
        }
        for y in 0..h {
            phi[idx(0, y, w)] = 0.0;
            phi[idx(w - 1, y, w)] = 0.0;
        }
        let mut sc = scratch_of(w, h, 0);
        // ★ 门线必须按 **f32 地板** 定（照搬 f64 的 1e-9 会永远不收敛）
        let opt = MgOptions { max_cycles: 40, ..MgOptions::f32_default() };
        let rep = solve_poisson_mg(&rho, &mut phi, w, h, &mut sc, &opt).unwrap();
        assert!(
            rep.converged,
            "应达门线：r={} floor={} cycles={}",
            rep.residual_inf, rep.roundoff_floor, rep.cycles
        );
        assert!(
            rep.residual_inf <= rep.roundoff_floor * 2.0,
            "残差应达到 f32 地板量级：r={} floor={}",
            rep.residual_inf,
            rep.roundoff_floor
        );
        let mut mx = 0.0f32;
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                mx = mx.max(fmath::abs_f32(phi[idx(x, y, w)]));
            }
        }
        assert!(mx < 1.0e-3, "齐次问题应收到 0，实测 max|V|={mx}");
    }

    /// ★ **主判据：制造解（manufactured solution）逐点对照。**
    ///
    /// 取连续解 `V(x,y) = sin(πx/W)·sin(πy/H)`（边框恰为 0 ⇒ 满足 Dirichlet）。
    /// 其 `∇²V = −π²(1/W² + 1/H²)·V` ⇒ 令 `ρ = −λ_c·V_exact`（**在网格上逐点算**）。
    ///
    /// **期望值来自理论**：五点差分对该特征函数的**离散特征值**
    /// `λ_d = 4 − 2cos(π/(W−1)) − 2cos(π/(H−1))`，连续值 `λ_c = π²(1/(W−1)² + 1/(H−1)²)`
    /// ⇒ 解幅度按 `λ_c/λ_d` 缩放。`W = H = 33`（**域长 32**）时 `≈ 1.00125` ⇒ **理论相对偏差 ≈ 1.25e-3**。
    /// ⚠️ **注意分母是 `W − 1` 而不是 `W`**：网格含**两端** Dirichlet 点 ⇒ 特征函数是
    /// `sin(πx/(W−1))`（在 `x = W−1` 处才归零）。我初版写成 `sin(πx/W)` ⇒ **参照系整体错**，
    /// 使"误差"读数完全没有意义（这是本轮最贵的一次自纠）。
    #[test]
    fn manufactured_solution_matches_continuous_within_theory() {
        let (w, h) = (33usize, 33usize);
        let n = (w - 1) as f32; // 域长（= 32）
        let pi = core::f32::consts::PI;
        let ve = |x: usize, y: usize| -> f32 {
            fmath::sin(pi * x as f32 / n) * fmath::sin(pi * y as f32 / n)
        };
        let lam_c = 2.0 * pi * pi / (n * n);
        let mut rho = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                rho[idx(x, y, w)] = -lam_c * ve(x, y);
            }
        }
        let mut phi = vec![0.0f32; w * h];
        let mut sc = scratch_of(w, h, 0);
        let opt = MgOptions { max_cycles: 60, ..MgOptions::f32_default() };
        let rep = solve_poisson_mg(&rho, &mut phi, w, h, &mut sc, &opt).unwrap();
        assert!(
            rep.converged,
            "未收敛：r={} floor={} cycles={} levels={}",
            rep.residual_inf, rep.roundoff_floor, rep.cycles, rep.levels
        );

        // ① 离散方程逐点满足（"解对了"的定义）
        let r = residual_inf(&phi, &rho, w, h, 1.0);
        assert!(r <= rep.roundoff_floor * 2.0, "离散残差应达 f32 地板量级：r={r} floor={}", rep.roundoff_floor);

        // ② 与连续解对照：偏差 ≤ 理论离散化偏差 × 3（余量含迭代残差与 f32 舍入）
        let lam_d = 4.0 - 2.0 * fmath::cos(pi / n) - 2.0 * fmath::cos(pi / n);
        let theory = fmath::abs_f32(1.0 - lam_c / lam_d);
        let mut max_err = 0.0f32;
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let d = fmath::abs_f32(phi[idx(x, y, w)] - ve(x, y));
                if d > max_err {
                    max_err = d;
                }
            }
        }
        println!(
            "[field·poisson] 制造解 levels={} cycles={} r1={:.3e} r={:.3e} 下降{:.1}× 偏差={:.3e} 理论={:.3e}",
            rep.levels,
            rep.cycles,
            rep.residual_after_first,
            rep.residual_inf,
            rep.reduction_per_cycle().unwrap_or(f32::NAN),
            max_err,
            theory
        );
        assert!(
            max_err <= theory * 3.0,
            "与连续解偏差 {max_err} 超出理论界 {}（theory={theory}）",
            theory * 3.0
        );
    }

    /// ★ **「为什么必须多重网格」的正向对照（工作量对齐、判据来自理论）。**
    ///
    /// 同题（**平滑源** —— MG 的设计工况，也是设计稿 §2.2.3(2) 复杂度表的适用域），
    /// 比较"到达 `f32` 地板"各自需要多少**工作量单位**：
    /// - **多重网格**：V-cycle 次数（每个 V-cycle ≈ `2·Σ len_l ≈ 2.7·N` 次格点更新）
    /// - **单层红黑 GS**：全扫次数（每次 = `N` 次格点更新）
    ///
    /// **判据来自理论**（不是拍脑袋的百分比）：理论结论是
    /// `O(n²)` 次全扫 vs `O(1)` 次 V-cycle ⇒ 比值应随 `n` 增长；`n = 32` 时取 **≥ 10 倍**作**保守下界**。
    /// ⚠️ 注意这里比的是"**到达同一残差**"，不是"同样次数"——后者会把 MG 每个 cycle 的额外工作量算漏。
    #[test]
    fn multigrid_needs_far_fewer_sweeps_than_single_level() {
        let (w, h) = (33usize, 33usize);
        let n = (w - 1) as f32;
        let pi = core::f32::consts::PI;
        let (lam_c, lam_d) = (2.0 * pi * pi / (n * n), 4.0 - 4.0 * fmath::cos(pi / n));
        let mut rho = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                rho[idx(x, y, w)] = -lam_c
                    * fmath::sin(pi * x as f32 / n)
                    * fmath::sin(pi * y as f32 / n);
            }
        }
        let _ = lam_d;
        // 地板按"解的量级 |V| ≤ 1"估（该特征函数的最大值为 1）
        let floor = roundoff_floor_f32(1.0);
        let target = floor * 2.0;

        // —— MG：最少几个 V-cycle 到达地板 ——
        let mut mg_cycles = 0u32;
        let mut log = String::new();
        let mut k = 1u32;
        while k <= 16 {
            let mut phi = vec![0.0f32; w * h];
            let mut sc = scratch_of(w, h, 0);
            let opt = MgOptions { tol: 0.0, max_cycles: k, ..MgOptions::f32_default() };
            let rep = solve_poisson_mg(&rho, &mut phi, w, h, &mut sc, &opt).unwrap();
            log.push_str(&format!("  MG V-cycle={k:<3} r={:.4e}\n", rep.residual_inf));
            if rep.residual_inf <= target {
                mg_cycles = k;
                break;
            }
            k += 1;
        }

        // —— 单层 GS：最少几次全扫到达地板（倍增搜索，总代价 ~2×）——
        let mut gs_full_sweeps = 0u32;
        let mut s = 1u32;
        while s <= 65_536 {
            let mut phi = vec![0.0f32; w * h];
            gs_sweeps(&mut phi, &rho, w, h, 1.0, s);
            let r = residual_inf(&phi, &rho, w, h, 1.0);
            log.push_str(&format!("  单层GS 全扫={s:<6} r={:.4e}\n", r));
            if r <= target {
                gs_full_sweeps = s;
                break;
            }
            s *= 2;
        }

        println!(
            "[field·poisson] 到达 f32 地板(2×{floor:.2e}) 所需：MG={mg_cycles} V-cycle，单层GS={gs_full_sweeps} 全扫 ⇒ 比值 {:.1}\n{log}",
            gs_full_sweeps as f32 / mg_cycles.max(1) as f32
        );
        assert!(mg_cycles > 0, "MG 应在 16 个 V-cycle 内到达 f32 地板");
        assert!(gs_full_sweeps > 0, "单层 GS 应在 65536 次全扫内到达 f32 地板（否则说明题面/门线有问题）");
        assert!(
            gs_full_sweeps >= 10 * mg_cycles,
            "单层 GS 所需全扫({gs_full_sweeps}) 应至少是 MG V-cycle 数({mg_cycles}) 的 10 倍（理论下界）"
        );
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let (w, h) = (17usize, 17usize);
        let rho: Vec<f32> = (0..w * h).map(|i| ((i % 7) as f32) * 0.1 - 0.3).collect();
        let mut a = vec![0.0f32; w * h];
        let mut b = vec![0.0f32; w * h];
        let mut sa = scratch_of(w, h, 0);
        let mut sb = scratch_of(w, h, 0);
        let opt = MgOptions { tol: 0.0, max_cycles: 3, ..Default::default() };
        let ra = solve_poisson_mg(&rho, &mut a, w, h, &mut sa, &opt).unwrap();
        let rb = solve_poisson_mg(&rho, &mut b, w, h, &mut sb, &opt).unwrap();
        assert_eq!(a, b, "同输入必须同输出（判据可复现性的前提）");
        assert_eq!(ra.residual_inf.to_bits(), rb.residual_inf.to_bits());
    }

    #[test]
    fn nan_does_not_silently_pass_gate() {
        assert!(max_abs(&[1.0f32, f32::NAN, 0.5]).is_nan(), "NaN 必须整体判 NaN（否则会静默过门线）");
        assert_eq!(max_abs(&[1.0f32, f32::INFINITY, 0.5]), f32::INFINITY);
        assert_eq!(max_abs(&[]), 0.0);
        assert_eq!(max_abs(&[-3.0f32, 2.0]), 3.0);
    }
}
