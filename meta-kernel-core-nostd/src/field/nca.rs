//! # 场方程求解器 · **NCA 局部规则**（纯算层子模块）
//!
//! 来源：`coordination/discussions/2026-09-19_场方程求解器设计稿.md`
//! §2.2.2(3)（NCA 结构，Mordvintsev et al., Distill 2020）、§2.2.3(1)（"三引擎的学习版"口径）、
//! §2.2.4（纯算层 §2）。
//!
//! ## 结构（对原论文的**简化**，逐项说明差异）
//!
//! ```text
//!   感知 Perception（固定核）      Sobel_x / Sobel_y 作用于每个通道       ← 与原论文一致
//!          ↓   P = concat(S, gx, gy)
//!   更新 Update（可学习）          h = ReLU(W1·P + b1);  ΔS = W2·h + b2   ← 与原论文同构
//!          ↓   S ← S + ΔS · mask · alive
//!   存活掩码 Alive                 maxpool₃ₓ₃(S[α]) > θ ⇒ 活                 ← 与原论文一致
//! ```
//!
//! | 项 | 原论文（2020） | **本层（简化版）** | 为什么 |
//! |---|---|---|---|
//! | 通道数 | **16** | **4**（[`CH`]） | 内存：16 通道 × 128² × 4B ＝ 1 MiB ⇒ 超出单帧预算；4 通道 × 128² × 4B ＝ 256 KiB 可控 |
//! | 隐藏维 | 128 | **16**（[`HIDDEN`]） | 参数表须以 `const` 进内核；192+16+64+4 ＝ 276 个 `f32` ≈ 1.1 KB，可静态嵌入 |
//! | 随机掩码 | 训练用 `p = 0.5` 丢弃 | **确定性 PRNG**（[`Xorshift32`]） | ★ **必须**：内核不得感知（机制 21：时钟/随机源皆属感知）＋ 判据必须可复现（E1） |
//! | 权重来源 | 训练得到 | **`const` 表**（本层只做**前向**） | ★ **训练不在内核内**（E2 / C1 零依赖） |
//!
//! ## 与「三引擎」的口径（用户裁定：**包含**，不是等同）
//!
//! 三引擎（`linear`／`fib`／`expo`）是**生成模型的固定权重特例**：
//! 取 `W1`＝单位、`W2`＝固定的"一步推进"矩阵、去掉掩码与存活规则 ⇒ 退回三引擎的确定性递推。
//! ⇒ **同一个概念只许一份实现**：本模块**不重实现**三引擎；节律由
//! [`crate::field::rhythm_steps`]**import** `engine_select` 得到（C19 同源）。
//!
//! ## `unsafe`／依赖
//!
//! 纯算术 ＋ 整数 PRNG ⇒ **0 处 `unsafe`、0 个新依赖**（crate 级 `#![forbid(unsafe_code)]`）。
//!
//! ## 数据布局（**SoA：按通道分平面**）
//!
//! 通道 `c` 的格点 `(x, y)` 在扁平数组中的下标 ＝ `c*w*h + y*w + x`。
//! 理由（设计稿 §2.2.3(4)）：SIMD 需要**同通道连续**；且各通道可作为独立数组参与多重网格。

// ⚠️ `fmath` 只在**测试**里用到（判据要按 ULP 比较）⇒ 加 `cfg(test)`，避免 `lib` 构建的 unused 警告。
#[cfg(test)]
use crate::fmath;

/// 通道数（简化版：论文是 16）。
pub const CH: usize = 4;
/// 感知维：`S`、`gx`、`gy` 各 `CH` 个 ⇒ `3·CH`。
pub const PERCEPTION: usize = CH * 3;
/// 隐藏层宽度（简化版：论文是 128）。
pub const HIDDEN: usize = 16;
/// 存活判据的默认阈值（论文用 `0.1` 作用于 `alpha` 通道）。
pub const ALIVE_THRESHOLD: f32 = 0.1;
/// 通道 0 约定为 `alpha`（存活判据就读它）。
pub const ALPHA: usize = 0;

/// **确定性 PRNG**（xorshift32）。
///
/// ★ **必须是确定性的**（E1）：① 内核**不得感知**（机制 21 —— 系统随机源属感知）；
/// ② 裸机无随机源；③ **判据要可复现**（同种子 ⇒ 同结果）。故**不提供**任何"用系统熵播种"的接口。
#[derive(Clone, Copy, Debug)]
pub struct Xorshift32 {
    s: u32,
}

impl Xorshift32 {
    /// 种子 **0 会被替换为 1**（xorshift 的 0 是不动点 ⇒ 会永远输出 0）。
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        Self { s: if seed == 0 { 0x9E37_79B9 } else { seed } }
    }

    #[must_use]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.s = x;
        x
    }

    /// `[0, 1)` 的 `f32`（取高 24 位 ⇒ 均匀、无偏）。
    #[must_use]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    /// 伯努利：以概率 `p` 返回 `true`。
    #[must_use]
    pub fn bernoulli(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }
}

/// NCA 权重表（**前向**用；训练在宿主机外，见 E2）。
#[derive(Clone, Copy, Debug)]
pub struct NcaWeights {
    /// `W1`：`HIDDEN × PERCEPTION`
    pub w1: [f32; HIDDEN * PERCEPTION],
    /// `b1`：`HIDDEN`
    pub b1: [f32; HIDDEN],
    /// `W2`：`CH × HIDDEN`
    pub w2: [f32; CH * HIDDEN],
    /// `b2`：`CH`
    pub b2: [f32; CH],
}

impl NcaWeights {
    /// 全零（⇒ `ΔS ≡ 0` ⇒ **恒等映射**）。
    #[must_use]
    pub const fn zeroed() -> Self {
        Self {
            w1: [0.0; HIDDEN * PERCEPTION],
            b1: [0.0; HIDDEN],
            w2: [0.0; CH * HIDDEN],
            b2: [0.0; CH],
        }
    }

    /// 论文式初始化：`W1` 小随机（**由确定性 PRNG 生成**）、`W2 = 0`、`b2 = 0`。
    ///
    /// `W2 = 0` 是论文的刻意设计：**初始状态是恒等映射** ⇒ 训练从"什么都不改"开始。
    #[must_use]
    pub fn paper_init(seed: u32) -> Self {
        let mut rng = Xorshift32::new(seed);
        let mut w = Self::zeroed();
        let mut i = 0;
        while i < w.w1.len() {
            w.w1[i] = rng.next_f32() * 0.2 - 0.1; // U(-0.1, 0.1)，与论文同量级
            i += 1;
        }
        w
    }
}

#[inline]
fn idx(x: usize, y: usize, w: usize) -> usize {
    y * w + x
}

/// 3×3 邻域箱式池化（**边界用复制填充**，确定性）。
pub fn maxpool3(src: &[f32], w: usize, h: usize, c: usize, out: &mut [f32]) -> usize {
    if w == 0 || h == 0 || src.len() < w * h {
        return 0;
    }
    let plane = &src[c * w * h..(c + 1) * w * h];
    let mut n = 0usize;
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        while x < w {
            if n >= out.len() {
                return n;
            }
            let mut m = f32::NEG_INFINITY;
            let mut dy = -1i32;
            while dy <= 1 {
                let mut dx = -1i32;
                while dx <= 1 {
                    let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let v = plane[idx(sx, sy, w)];
                    if v > m {
                        m = v;
                    }
                    dx += 1;
                }
                dy += 1;
            }
            out[n] = m;
            n += 1;
            x += 1;
        }
        y += 1;
    }
    n
}

/// Sobel 感知：把通道 `c` 的 `gx`／`gy` 写入调用方缓冲（**边界复制填充**、确定性）。
pub fn sobel(src: &[f32], w: usize, h: usize, c: usize, gx: &mut [f32], gy: &mut [f32]) -> usize {
    if w == 0 || h == 0 || src.len() < CH * w * h {
        return 0;
    }
    let plane = &src[c * w * h..(c + 1) * w * h];
    let at = |x: i32, y: i32| -> f32 {
        let sx = x.clamp(0, w as i32 - 1) as usize;
        let sy = y.clamp(0, h as i32 - 1) as usize;
        plane[idx(sx, sy, w)]
    };
    let mut n = 0usize;
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        while x < w {
            if n >= gx.len() || n >= gy.len() {
                return n;
            }
            let (xi, yi) = (x as i32, y as i32);
            // 经典 Sobel 核
            let l = -at(xi - 1, yi - 1) - 2.0 * at(xi - 1, yi) - at(xi - 1, yi + 1);
            let r = at(xi + 1, yi - 1) + 2.0 * at(xi + 1, yi) + at(xi + 1, yi + 1);
            let t = -at(xi - 1, yi - 1) - 2.0 * at(xi, yi - 1) - at(xi + 1, yi - 1);
            let b = at(xi - 1, yi + 1) + 2.0 * at(xi, yi + 1) + at(xi + 1, yi + 1);
            gx[n] = r + l;
            gy[n] = b + t;
            n += 1;
            x += 1;
        }
        y += 1;
    }
    n
}

/// **存活掩码**：`alive[i] = maxpool₃ₓ₃(S[alpha])[i] > theta`。
pub fn alive_mask(src: &[f32], w: usize, h: usize, theta: f32, pool_scratch: &mut [f32], out: &mut [f32]) -> usize {
    let n = maxpool3(src, w, h, ALPHA, pool_scratch);
    let mut i = 0usize;
    while i < n {
        if i >= out.len() {
            return i;
        }
        out[i] = if pool_scratch[i] > theta { 1.0 } else { 0.0 };
        i += 1;
    }
    n
}

/// 感知缓冲大小（`f32` 个数）：`PERCEPTION × w × h`。
#[must_use]
pub const fn perception_len(w: usize, h: usize) -> usize {
    PERCEPTION * w * h
}

/// **一步局部更新**：`dst = alive ⊙ (src + ΔS ⊙ mask)`。
///
/// 参数：`src`／`dst`（各 `CH*w*h`，SoA）、`weights`、`rng`（确定性）、`mask_p`（丢弃概率）、
/// `percept`／`hidden`／`pool`／`alive` 为**调用方提供的暂存**（避免 `alloc`）。
///
/// **返回**：实际处理并写出的格点数（缓冲不足则提前返回，**不 panic**）。
#[allow(clippy::too_many_arguments)]
pub fn step(
    src: &[f32],
    dst: &mut [f32],
    w: usize,
    h: usize,
    weights: &NcaWeights,
    rng: &mut Xorshift32,
    mask_p: f32,
    percept: &mut [f32],
    hidden: &mut [f32],
    pool: &mut [f32],
    alive: &mut [f32],
) -> usize {
    let cells = w * h;
    if cells == 0 || src.len() < CH * cells || dst.len() < CH * cells {
        return 0;
    }
    // ① 感知：P = concat(S, Sobel_x(c), Sobel_y(c))
    //    布局：平面 0..CH = S；CH..2CH = gx；2CH..3CH = gy。
    //    ⚠️ 先把缓冲切成三段，避免"同一缓冲两个可变借用"（NLL 下也不该赌侥幸）。
    if percept.len() < perception_len(w, h) {
        return 0;
    }
    {
        let (s_sec, rest) = percept.split_at_mut(CH * cells);
        let (gx_sec, gy_sec) = rest.split_at_mut(CH * cells);
        let mut c = 0usize;
        while c < CH {
            let r = c * cells..(c + 1) * cells;
            s_sec[r.clone()].copy_from_slice(&src[c * cells..(c + 1) * cells]);
            sobel(src, w, h, c, &mut gx_sec[r.clone()], &mut gy_sec[r]);
            c += 1;
        }
    }

    // ② 更新：ΔS = W2 · ReLU(W1·P + b1) + b2（逐格）
    if hidden.len() < HIDDEN {
        return 0;
    }
    let mut delta = [0.0f32; CH];
    let mut i = 0usize;
    while i < cells {
        let mut k = 0usize;
        while k < HIDDEN {
            let mut acc = weights.b1[k];
            let mut j = 0usize;
            while j < PERCEPTION {
                acc += weights.w1[k * PERCEPTION + j] * percept[j * cells + i];
                j += 1;
            }
            hidden[k] = if acc > 0.0 { acc } else { 0.0 };
            k += 1;
        }
        let mut o = 0usize;
        while o < CH {
            let mut acc = weights.b2[o];
            let mut k = 0usize;
            while k < HIDDEN {
                acc += weights.w2[o * HIDDEN + k] * hidden[k];
                k += 1;
            }
            delta[o] = acc;
            o += 1;
        }
        let mut c = 0usize;
        while c < CH {
            dst[c * cells + i] = delta[c];
            c += 1;
        }
        i += 1;
    }

    // ④ 存活掩码（读 src 的 alpha 平面）
    if alive.len() < cells || pool.len() < cells {
        return 0;
    }
    alive_mask(src, w, h, ALIVE_THRESHOLD, pool, alive);

    // ⑤ 合成：dst = alive ⊙ (src + ΔS ⊙ mask)
    let mut i = 0usize;
    while i < cells {
        // mask：以概率 `mask_p` **跳过**该格（论文训练用 0.5）
        let keep = if mask_p <= 0.0 { true } else { !rng.bernoulli(mask_p) };
        let a = alive[i];
        let mut c = 0usize;
        while c < CH {
            let o = c * cells + i;
            let s = src[o];
            let dd = dst[o];
            dst[o] = if keep { a * (s + dd) } else { a * s };
            c += 1;
        }
        i += 1;
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng_cell_field(w: usize, h: usize, seed: u32) -> Vec<f32> {
        let mut r = Xorshift32::new(seed);
        (0..CH * w * h).map(|_| r.next_f32()).collect()
    }

    /// ★ **PRNG 必须确定性**（E1）——同种子同序列、值域正确、种子 0 不退化。
    #[test]
    fn prng_is_deterministic_and_sane() {
        let mut a = Xorshift32::new(42);
        let mut b = Xorshift32::new(42);
        let mut n = 0;
        while n < 1000 {
            let x = a.next_u32();
            assert_eq!(x, b.next_u32(), "同种子必须同序列");
            assert_eq!(Xorshift32::new(7).next_u32(), Xorshift32::new(7).next_u32());
            n += 1;
        }
        assert_ne!(Xorshift32::new(1).next_u32(), Xorshift32::new(2).next_u32(), "不同种子应不同");
        // 种子 0 会被替换 ⇒ 不退化
        assert_ne!(Xorshift32::new(0).next_u32(), 0, "种子 0 不得退化为恒 0");
        // 值域
        let mut r = Xorshift32::new(9);
        let mut i = 0;
        while i < 10_000 {
            let v = r.next_f32();
            assert!((0.0..1.0).contains(&v), "next_f32 必在 [0,1)，实测 {v}");
            i += 1;
        }
        // 伯努利频率（粗检；p=0.5 的 4σ 界）
        let mut r2 = Xorshift32::new(123);
        let mut k = 0u32;
        let total = 20_000u32;
        for _ in 0..total {
            if r2.bernoulli(0.5) {
                k += 1;
            }
        }
        let f = k as f32 / total as f32;
        assert!((f - 0.5).abs() < 0.05, "p=0.5 的频率应 ≈0.5，实测 {f}");
    }

    /// ★ **论文的关键性质**：`W2 = 0`、`b2 = 0` ⇒ `ΔS ≡ 0` ⇒ **一步之后状态不变**。
    ///
    /// 这条既是"实现对不对"的判据，也是"训练为何能从恒等开始"的依据。
    #[test]
    fn zero_w2_gives_identity_step() {
        let (w, h) = (8usize, 8usize);
        let cells = w * h;
        // alpha 平面全 1 ⇒ 全部存活（隔离存活掩码的影响）
        let mut src = rng_cell_field(w, h, 5);
        for i in 0..cells {
            src[ALPHA * cells + i] = 1.0;
        }
        let mut dst = vec![0.0f32; CH * cells];
        let wt = NcaWeights::paper_init(77); // W2 = 0
        let mut rng = Xorshift32::new(1);
        let mut percept = vec![0.0f32; perception_len(w, h)];
        let mut hidden = vec![0.0f32; HIDDEN];
        let mut pool = vec![0.0f32; cells];
        let mut alive = vec![0.0f32; cells];
        let n = step(
            &src, &mut dst, w, h, &wt, &mut rng, 0.5, &mut percept, &mut hidden, &mut pool, &mut alive,
        );
        assert_eq!(n, cells);
        for i in 0..CH * cells {
            assert!(
                (dst[i] - src[i]).abs() < 1.0e-6,
                "W2=0 ⇒ ΔS=0 ⇒ 状态必须不变（i={i} src={} dst={}）",
                src[i],
                dst[i]
            );
        }
        // 全零权重同理
        let n2 = step(
            &src, &mut dst, w, h, &NcaWeights::zeroed(), &mut rng, 0.5, &mut percept, &mut hidden, &mut pool,
            &mut alive,
        );
        assert_eq!(n2, cells);
        assert!((0..CH * cells).all(|i| (dst[i] - src[i]).abs() < 1.0e-6));
    }

    /// Sobel：**竖直台阶** ⇒ `gx` 只在台阶列非零、`gy ≈ 0`（可手算的判据）。
    #[test]
    fn sobel_detects_vertical_edge() {
        let (w, h) = (6usize, 1usize);
        // 3 通道其余为 0；通道 0 左半 0、右半 1 ⇒ 台阶在 x=3 处
        let mut src = vec![0.0f32; CH * w * h];
        for x in 0..w {
            src[ALPHA * w * h + x] = if x >= 3 { 1.0 } else { 0.0 };
        }
        let mut gx = vec![0.0f32; w * h];
        let mut gy = vec![0.0f32; w * h];
        assert_eq!(sobel(&src, w, h, ALPHA, &mut gx, &mut gy), w * h);
        // 台阶处（x=3）：右邻全 1、左邻全 0 ⇒ gx = 4
        assert!((gx[3] - 4.0).abs() < 1.0e-5, "台阶处 gx 应为 +4，实测 {}", gx[3]);
        // 远离台阶处（x=5，右邻复制填充 = 1）⇒ gx = 0
        assert!(fmath::abs_f32(gx[5]) < 1.0e-5, "平坦区 gx 应为 0，实测 {}", gx[5]);
        // 竖直台阶 ⇒ gy 处处为 0
        for v in &gy {
            assert!(fmath::abs_f32(*v) < 1.0e-5, "竖直台阶上 gy 应为 0，实测 {v}");
        }
    }

    /// 存活掩码：**孤立的 alpha 单元**（3×3 邻域全 0）⇒ 被清零。
    #[test]
    fn alive_mask_kills_isolated_cell() {
        let (w, h) = (7usize, 7usize);
        let cells = w * h;
        let mut src = vec![0.0f32; CH * cells];
        // 中心点 alpha = 1（孤立；邻居都是 0）
        src[ALPHA * cells + idx(3, 3, w)] = 1.0;
        let mut pool = vec![0.0f32; cells];
        let mut alive = vec![0.0f32; cells];
        assert_eq!(alive_mask(&src, w, h, ALIVE_THRESHOLD, &mut pool, &mut alive), cells);
        // 孤立点的 maxpool = 1 ⇒ **仍然存活**（存活判据只看自身邻域最大值）
        assert_eq!(alive[idx(3, 3, w)], 1.0, "maxpool=1 ⇒ 存活（判据是邻域最大值，而非「是否孤立」）");
        // 远离它的点 maxpool = 0 ⇒ 死
        assert_eq!(alive[idx(0, 0, w)], 0.0);
        // 造一个 3×3 的块 ⇒ 整块存活（含中心）
        src[ALPHA * cells + idx(1, 1, w)] = 1.0;
        src[ALPHA * cells + idx(2, 2, w)] = 1.0;
        assert_eq!(alive_mask(&src, w, h, ALIVE_THRESHOLD, &mut pool, &mut alive), cells);
        assert_eq!(alive[idx(1, 1, w)], 1.0);
        assert_eq!(alive[idx(0, 0, w)], 1.0, "(0,0) 的邻域含 (1,1) ⇒ 也被激活");
    }

    #[test]
    fn maxpool_is_local_max_and_deterministic() {
        let (w, h) = (3usize, 3usize);
        let cells = w * h;
        let mut src = vec![0.0f32; CH * cells];
        src[ALPHA * cells + 4] = 5.0; // 中心
        let mut out = vec![0.0f32; cells];
        assert_eq!(maxpool3(&src, w, h, ALPHA, &mut out), cells);
        // 3×3 网格里每个点的邻域都包含中心 ⇒ 全部 = 5
        assert!(out.iter().all(|&v| v == 5.0));
        // 短缓冲不 panic
        let mut tiny = vec![0.0f32; 2];
        assert_eq!(maxpool3(&src, w, h, ALPHA, &mut tiny), 2);
    }

    /// `step` 的整体性质：**确定性**（同种子同结果）、**有限值**、短缓冲不 panic。
    #[test]
    fn step_is_deterministic_finite_and_safe() {
        let (w, h) = (8usize, 8usize);
        let cells = w * h;
        let mut src = rng_cell_field(w, h, 11);
        for i in 0..cells {
            src[ALPHA * cells + i] = 1.0;
        }
        // 给 W2 一点非零权重 ⇒ ΔS 不为 0（否则测不出东西）
        let mut wt = NcaWeights::paper_init(3);
        let mut k = 0;
        while k < wt.w2.len() {
            wt.w2[k] = 0.01;
            k += 1;
        }
        let run = || -> Vec<f32> {
            let mut dst = vec![0.0f32; CH * cells];
            let mut rng = Xorshift32::new(2);
            let mut percept = vec![0.0f32; perception_len(w, h)];
            let mut hidden = vec![0.0f32; HIDDEN];
            let mut pool = vec![0.0f32; cells];
            let mut alive = vec![0.0f32; cells];
            let n = step(&src, &mut dst, w, h, &wt, &mut rng, 0.5, &mut percept, &mut hidden, &mut pool, &mut alive);
            assert_eq!(n, cells);
            dst
        };
        let a = run();
        let b = run();
        assert_eq!(a, b, "同种子必须同结果");
        assert!(a.iter().all(|v| v.is_finite()), "结果必须全为有限值");
        assert!(a.iter().any(|v| (v - 0.0).abs() > 1.0e-9), "ΔS≠0 ⇒ 状态应该真的变了");

        // 缓冲不足 ⇒ 返回 0（不 panic）
        let mut short_p = vec![0.0f32; 3];
        let mut dst = vec![0.0f32; CH * cells];
        let mut rng = Xorshift32::new(2);
        let mut hidden = vec![0.0f32; HIDDEN];
        let mut pool = vec![0.0f32; cells];
        let mut alive = vec![0.0f32; cells];
        assert_eq!(
            step(&src, &mut dst, w, h, &wt, &mut rng, 0.5, &mut short_p, &mut hidden, &mut pool, &mut alive),
            0
        );
        assert_eq!(step(&[], &mut dst, 0, 0, &wt, &mut rng, 0.5, &mut short_p, &mut hidden, &mut pool, &mut alive), 0);
    }

    /// 掩码概率的**可观测效应**：`mask_p = 1.0` ⇒ 全部跳过 ⇒ 等价恒等；`mask_p = 0.0` ⇒ 全部更新。
    #[test]
    fn mask_probability_has_expected_extremes() {
        let (w, h) = (6usize, 6usize);
        let cells = w * h;
        let mut src = rng_cell_field(w, h, 21);
        for i in 0..cells {
            src[ALPHA * cells + i] = 1.0;
        }
        let mut wt = NcaWeights::paper_init(3);
        let mut k = 0;
        while k < wt.w2.len() {
            wt.w2[k] = 0.05;
            k += 1;
        }
        let mut percept = vec![0.0f32; perception_len(w, h)];
        let mut hidden = vec![0.0f32; HIDDEN];
        let mut pool = vec![0.0f32; cells];
        let mut alive = vec![0.0f32; cells];
        let mut dst = vec![0.0f32; CH * cells];

        // mask_p = 1.0 ⇒ 全跳过 ⇒ dst == src
        let mut rng = Xorshift32::new(4);
        step(&src, &mut dst, w, h, &wt, &mut rng, 1.0, &mut percept, &mut hidden, &mut pool, &mut alive);
        for i in 0..CH * cells {
            assert!((dst[i] - src[i]).abs() < 1.0e-6, "mask_p=1 ⇒ 应全跳过");
        }
        // mask_p = 0.0 ⇒ 全更新 ⇒ 至少有一格变了
        let mut rng2 = Xorshift32::new(4);
        step(&src, &mut dst, w, h, &wt, &mut rng2, 0.0, &mut percept, &mut hidden, &mut pool, &mut alive);
        assert!(
            (0..CH * cells).any(|i| (dst[i] - src[i]).abs() > 1.0e-9),
            "mask_p=0 ⇒ 应全更新"
        );
    }
}
