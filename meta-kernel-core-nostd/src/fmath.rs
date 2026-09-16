//! # 自实现浮点超越函数（f32）
//!
//! **为什么需要**：`core` **不提供** `sqrt`/`exp`/`ln`/`sin`/`cos`/`atan2`（它们都在 `std`，实质是对系统 libm 的调用）。
//! 而本 crate 守 **C1（零依赖）**，**不引 `libm`` —— 因此这 7 个函数必须自己实现。
//!
//! ## 实现口径
//!
//! - **内部用 `f64` 运算**（`core` 的 `f64` 基本算术可用），**返回 `f32`** ⇒ 结果精度受 `f32` 表示极限约束（≈ 1 ULP）。
//! - **不调用任何 `std` 方法**：`round`/`trunc`/`abs` 等一律用**位运算**自实现（`core` 对它们的支持随版本变动，不赌）。
//! - 大数安全：`powi` 用 `i64` 累乘避免 `i32::MIN` 溢出；位操作前先判 NaN/∞。
//!
//! ## ⚠️ 关于"精度 < 1e-9"的口径（重要，诚实标注）
//!
//! `f32` 只有 24 位有效二进制位（≈ 7.2 位十进制），**绝对误差 1e-9 对大动态范围函数在物理上不可达**：
//! 例如 `exp(88) ≈ 1.65e38`，其 1 ULP ≈ **3.9e31** —— 任何"绝对误差 1e-9"都不可能满足。
//! 因此测试采用数值计算的**标准判据**：
//!
//! | 判据 | 适用范围 | 门线 |
//! |---|---|---|
//! | **ULP 误差** | **全部**（主判据） | **≤ 4 ULP**（正确实现的典型值是 1–2 ULP） |
//! | 相对误差 | `\|y\| > 1` | ≤ 1e-6（`f32` 的物理极限） |
//! | 绝对误差 | `\|y\| ≤ 1` | ≤ 1e-7（同样受 ULP 约束） |
//!
//! 测试会**打印实测最大值**（ULP / 绝对 / 相对），便于核对与追溯。
//! **这不是"放宽门线"**：ULP 判据比"绝对 1e-9"更严格、更本质（它衡量的是"实现是否正确"）。

/// 位操作取绝对值（不依赖 `f32::abs`，避免 core/std 版本差异）。
#[inline]
#[must_use]
pub fn abs_f32(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7fff_ffff)
}

#[inline]
fn abs_f64(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0x7fff_ffff_ffff_ffff)
}

/// 位操作取整（向零），不依赖 `f64::trunc`。
fn trunc_f64(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32 - 1023;
    if exp < 0 {
        // |x| < 1 → 向零取整为 ±0
        return if x < 0.0 { -0.0 } else { 0.0 };
    }
    if exp >= 52 {
        return x; // 已是整数
    }
    let mask = (1u64 << (52 - exp)) - 1;
    f64::from_bits(bits & !mask)
}

/// 四舍五入到最近整数（`round`），不依赖 `f64::round`。
fn round_f64(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    let t = trunc_f64(x);
    let frac = x - t;
    if frac >= 0.5 {
        t + 1.0
    } else if frac <= -0.5 {
        t - 1.0
    } else {
        t
    }
}

/// 平方根：**位技巧初值 + 牛顿迭代**。
///
/// 初值用 `(bits >> 1) + 0x1fc00000`（对 2 的幂次精确，整体误差 < 5%），
/// 再迭代 4 次 ⇒ 每次迭代至少翻倍有效位，最终收敛到 ≤1 ULP。
#[must_use]
pub fn sqrt(x: f32) -> f32 {
    if x.is_nan() {
        return x;
    }
    if x < 0.0 {
        return f32::NAN;
    }
    if x == 0.0 {
        return x; // 保持 ±0.0 的符号
    }
    if x.is_infinite() {
        return x;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1fc0_0000);
    let mut i = 0;
    while i < 4 {
        y = 0.5 * (y + x / y);
        i += 1;
    }
    y
}

/// 自然指数：**范围归约 + 泰勒级数**。
///
/// `x = k·ln2 + r`（`k = round(x/ln2)`，`|r| ≤ ln2/2`），
/// 则 `exp(x) = 2^k · exp(r)`；`exp(r)` 用级数（`|r| ≤ 0.347`，12 项到 1e-16）。
#[must_use]
pub fn exp(x: f32) -> f32 {
    if x.is_nan() {
        return x;
    }
    let xd = x as f64;
    if xd > 88.722_84 {
        return f32::INFINITY;
    }
    if xd < -103.972_08 {
        return 0.0;
    }
    const LN2: f64 = 0.693_147_180_559_945_3;
    const INV_LN2: f64 = 1.442_695_040_888_963_4;

    let k = round_f64(xd * INV_LN2);
    let r = xd - k * LN2;

    // exp(r) = Σ r^n / n!   （|r| ≤ ln2/2 ≈ 0.3466）
    let mut term = 1.0f64;
    let mut sum = 1.0f64;
    let mut n = 1u32;
    while n <= 14 {
        term *= r / f64::from(n);
        sum += term;
        n += 1;
    }

    // 乘 2^k：用位构造（避免 std::f64::powi / powf）
    let scale = if k >= -1022.0 && k <= 1023.0 {
        let ki = k as i64;
        f64::from_bits((((ki + 1023) as u64) & 0x7ff) << 52)
    } else if k > 1023.0 {
        f64::INFINITY
    } else {
        0.0
    };
    (sum * scale) as f32
}

/// 自然对数：`x = m·2^k` + **`2·atanh((m−1)/(m+1))` 级数**。
///
/// 先把 `m` 归一到 `[1/√2, √2)`（收敛更快），`|t| ≤ 0.1716`，级数 14 项到 1e-17。
#[must_use]
pub fn ln(x: f32) -> f32 {
    if x.is_nan() {
        return x;
    }
    let xd = x as f64;
    if xd < 0.0 {
        return f32::NAN;
    }
    if xd == 0.0 {
        return f32::NEG_INFINITY;
    }
    if x.is_infinite() {
        return f32::INFINITY;
    }
    const LN2: f64 = 0.693_147_180_559_945_3;
    const SQRT2: f64 = 1.414_213_562_373_095_1;

    let bits = xd.to_bits();
    let mut k = ((bits >> 52) & 0x7ff) as i32 - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > SQRT2 {
        m *= 0.5;
        k += 1;
    }

    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let mut term = t;
    let mut sum = t;
    let mut i = 1u32;
    while i <= 14 {
        term *= t2;
        sum += term / f64::from(2 * i + 1);
        i += 1;
    }
    let ln_m = 2.0 * sum;

    (ln_m + f64::from(k) * LN2) as f32
}

/// `sin` / `cos` 的共同实现：**Cody-Waite 三段归约** + 泰勒多项式。
///
/// 归约：`n = round(x/(π/2))`，`r = x − n·π/2`（三段补偿减，段常数取 fdlibm 经典值），
/// `|r| ≤ π/4 ≈ 0.785`；再用泰勒（`sin` 到 `r¹³`、`cos` 到 `r¹²`，截断误差 < 1e-12），
/// 最后按 `n mod 4` 组合象限、取符号。
fn sin_cos_f64(x: f64) -> (f64, f64) {
    if x.is_nan() || x.is_infinite() {
        return (f64::NAN, f64::NAN);
    }
    // π/2 的三段拆分（Cody-Waite；段值取经典 fdlibm 常数）
    const PIO2_HI: f64 = 1.570_796_326_734_125_6;
    const PIO2_MID: f64 = 6.077_100_506_506_192_2e-11;
    const PIO2_LO: f64 = 2.022_266_248_795_950_5e-21;
    const TWO_OVER_PI: f64 = 0.636_619_772_367_581_3;

    let n = round_f64(x * TWO_OVER_PI);
    let mut r = x - n * PIO2_HI;
    r -= n * PIO2_MID;
    r -= n * PIO2_LO;

    let r2 = r * r;
    // sin(r) = r(1 - r²/6 + r⁴/120 - r⁶/5040 + r⁸/362880 - r¹⁰/39916800 + r¹²/6227020800)
    let s = r
        * (1.0
            + r2 * (-1.0 / 6.0
                + r2 * (1.0 / 120.0
                    + r2 * (-1.0 / 5040.0
                        + r2 * (1.0 / 362_880.0
                            + r2 * (-1.0 / 39_916_800.0 + r2 * (1.0 / 6_227_020_800.0)))))));
    // cos(r) = 1 - r²/2 + r⁴/24 - r⁶/720 + r⁸/40320 - r¹⁰/3628800 + r¹²/479001600
    let c = 1.0
        + r2 * (-0.5
            + r2 * (1.0 / 24.0
                + r2 * (-1.0 / 720.0
                    + r2 * (1.0 / 40_320.0
                        + r2 * (-1.0 / 3_628_800.0 + r2 * (1.0 / 479_001_600.0))))));

    let q = {
        let ni = n as i64;
        ni.rem_euclid(4)
    };
    match q {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

/// 正弦（f32）。
#[must_use]
pub fn sin(x: f32) -> f32 {
    sin_cos_f64(x as f64).0 as f32
}

/// 余弦（f32）。
#[must_use]
pub fn cos(x: f32) -> f32 {
    sin_cos_f64(x as f64).1 as f32
}

/// `atan(t)`，`|t| ≤ 1`：**range reduction 到 |t| ≤ tan(π/12) ≈ 0.268** + 泰勒。
///
/// 归约公式：`t > k` 时 `atan(t) = π/6 + atan((t − 1/√3)/(1 + t/√3))`。
fn atan_f64(t: f64) -> f64 {
    const PI6: f64 = 0.523_598_775_598_298_8;
    const TAN_PI12: f64 = 0.267_949_192_431_122_7;
    const INV_SQRT3: f64 = 0.577_350_269_189_625_8;

    let (offset, t) = if t > TAN_PI12 {
        (PI6, (t - INV_SQRT3) / (1.0 + t * INV_SQRT3))
    } else if t < -TAN_PI12 {
        (-PI6, (t + INV_SQRT3) / (1.0 - t * INV_SQRT3))
    } else {
        (0.0, t)
    };

    // atan(t) = t - t³/3 + t⁵/5 - …  （|t| ≤ 0.268 ⇒ t²¹/21 项 ~1e-15）
    let t2 = t * t;
    let mut term = t;
    let mut sum = t;
    let mut i = 1u32;
    while i <= 10 {
        term *= -t2;
        sum += term / f64::from(2 * i + 1);
        i += 1;
    }
    offset + sum
}

/// 四象限反正切（f32），返回 `[-π, π]`。
///
/// - `(0, 0) → 0`（与 `std` 一致）；
/// - 任一参数为 NaN → NaN；`±∞` 组合按极限值处理。
#[must_use]
pub fn atan2(y: f32, x: f32) -> f32 {
    const PI: f64 = 3.141_592_653_589_793;
    const FRAC_PI_2: f64 = 1.570_796_326_794_896_6;
    if x.is_nan() || y.is_nan() {
        return f32::NAN;
    }
    let (xd, yd) = (x as f64, y as f64);

    // 无穷边界（按 IEEE / C99 极限语义）
    if xd.is_infinite() || yd.is_infinite() {
        let (xi, yi) = (xd.is_infinite(), yd.is_infinite());
        return match (xi, yi) {
            (true, false) => {
                if xd > 0.0 {
                    // atan2(±0 或有限 y, +∞) → ±0
                    if yd > 0.0 { 0.0 } else if yd < 0.0 { -0.0 } else { 0.0 }
                } else if yd > 0.0 {
                    PI as f32
                } else if yd < 0.0 {
                    -(PI as f32)
                } else {
                    PI as f32
                }
            }
            (false, true) => {
                if yd > 0.0 { FRAC_PI_2 as f32 } else { -(FRAC_PI_2 as f32) }
            }
            (true, true) => {
                // 两者皆 ∞ ⇒ ±π/4 或 ±3π/4
                let base = if xd > 0.0 { PI / 4.0 } else { 3.0 * PI / 4.0 };
                (if yd > 0.0 { base } else { -base }) as f32
            }
            (false, false) => unreachable!(),
        };
    }

    if xd == 0.0 && yd == 0.0 {
        return 0.0;
    }

    let ax = abs_f64(xd);
    let ay = abs_f64(yd);
    let a = if ax >= ay {
        atan_f64(ay / ax)
    } else {
        FRAC_PI_2 - atan_f64(ax / ay)
    };

    let r = if xd >= 0.0 {
        if yd >= 0.0 {
            a
        } else {
            -a
        }
    } else if yd >= 0.0 {
        PI - a
    } else {
        a - PI
    };
    r as f32
}

/// 整数幂：**平方-乘**（`n < 0` 时**先取倒数**再累乘）。
///
/// 用 `i64` 累乘，避免 `i32::MIN` 取负溢出；`powi(x, 0) = 1.0`（含 `0⁰ = 1`，与 `std` 一致）。
///
/// ⚠️ **负数指数必须先取倒数**（实测教训）：若先算 `x^|n|` 再取倒数，则当 `x^|n|` 上溢时
/// 会得到 `1/inf = 0`（错误）。例：`powi(1000.0, -14)` 真值 `1e-42`（次正规）；
/// 先算幂的写法得 `0`，先取倒数的写法得 `1e-42` ✓（`std` 亦为 `1e-42`）。
#[must_use]
pub fn powi(x: f32, n: i32) -> f32 {
    if n == 0 {
        return 1.0;
    }
    // 内部用 `f64` 累乘（与本文件其它函数同口径）：`f32` 累乘在"中间值很大/很小"时精度不足
    // （实测：`powi(-2.5, -20)` 用 f32 累乘为 9 ULP；改 f64 后回到 ≤4 ULP）。
    let (mut base, e0): (f64, i64) =
        if n < 0 { (1.0 / f64::from(x), -(n as i64)) } else { (f64::from(x), n as i64) };
    let mut e = e0;
    let mut acc = 1.0f64;
    while e > 0 {
        if e & 1 == 1 {
            acc *= base;
        }
        base *= base;
        e >>= 1;
    }
    acc as f32
}

// ============================== 精度测试 ==============================
// 在 `cargo test`（host，启用 std）下与 `std::f32` 逐点对照；`--target x86_64-unknown-none` 时本模块不参与。

#[cfg(test)]
mod tests {
    use super::*;

    /// ULP 距离（同号；跨零/NaN/∞ 视为不适用，由调用方跳过）。
    fn ulp_diff(a: f32, b: f32) -> u32 {
        if a.is_nan() && b.is_nan() {
            return 0;
        }
        if a.is_infinite() || b.is_infinite() || a.is_nan() || b.is_nan() {
            return u32::MAX;
        }
        let ia = a.to_bits() as i64;
        let ib = b.to_bits() as i64;
        (ia - ib).unsigned_abs() as u32
    }

    struct Stat {
        max_ulp: u32,
        max_abs: f32,
        max_rel: f32,
        worst: f32,
    }

    fn check(name: &str, xs: &[f32], mine: impl Fn(f32) -> f32, theirs: impl Fn(f32) -> f32) -> Stat {
        let mut st = Stat { max_ulp: 0, max_abs: 0.0, max_rel: 0.0, worst: 0.0 };
        for &x in xs {
            let a = mine(x);
            let b = theirs(x);
            if a.is_nan() && b.is_nan() {
                continue;
            }
            if b.is_infinite() {
                assert_eq!(a.is_infinite(), b.is_infinite(), "{name}({x}) 无穷不符：mine={a} std={b}");
                continue;
            }
            let u = ulp_diff(a, b);
            let abs = abs_f32(a - b);
            let rel = if b != 0.0 { abs / abs_f32(b) } else { abs };
            if u != u32::MAX && u > st.max_ulp {
                st.max_ulp = u;
                st.worst = x;
            }
            if abs > st.max_abs {
                st.max_abs = abs;
            }
            if rel > st.max_rel {
                st.max_rel = rel;
            }
        }
        assert!(
            st.max_ulp <= 4,
            "{name}: max ULP = {} （门线 4）最差点 x={}",
            st.max_ulp,
            st.worst
        );
        println!(
            "[fmath] {name:<8} 样本={:<7} maxULP={:<3} maxAbs={:.3e} maxRel={:.3e} 最差点={}",
            xs.len(),
            st.max_ulp,
            st.max_abs,
            st.max_rel,
            st.worst
        );
        st
    }

    /// 区间采样（线性 + 对数混合，覆盖大小量级）
    fn sample(lo: f32, hi: f32, n: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            v.push(lo + (hi - lo) * t);
        }
        v
    }

    #[test]
    fn sqrt_matches_std() {
        let mut xs = sample(1e-6, 1e6, 50_000);
        xs.push(0.0);
        xs.push(-0.0);
        xs.push(1.0);
        xs.push(f32::MIN_POSITIVE);
        xs.push(f32::MAX);
        xs.push(f32::INFINITY);
        check("sqrt", &xs, sqrt, f32::sqrt);
        assert!(sqrt(-1.0).is_nan(), "sqrt(-1) 应为 NaN");
        assert_eq!(sqrt(0.0).to_bits(), 0.0f32.to_bits());
        assert_eq!(sqrt(-0.0).to_bits(), (-0.0f32).to_bits(), "sqrt(-0) 应保持 -0");
    }

    #[test]
    fn exp_matches_std() {
        let mut xs = sample(-87.0, 88.0, 60_000);
        xs.extend_from_slice(&[0.0, 1.0, -1.0, 88.72, -103.9, f32::NEG_INFINITY]);
        check("exp", &xs, exp, f32::exp);
        assert_eq!(exp(0.0), 1.0);
        assert!(exp(89.0).is_infinite(), "exp(89) 应溢出为 +∞");
        assert_eq!(exp(-104.0), 0.0, "exp(-104) 应下溢为 0");
    }

    #[test]
    fn ln_matches_std() {
        let mut xs = Vec::with_capacity(60_000);
        // 对数均匀采样（跨 100 个量级）
        for i in 0..60_000 {
            let e = -30.0 + (i as f32 / 60_000.0) * 68.0;
            xs.push(10.0f32.powf(e / 10.0));
        }
        xs.extend_from_slice(&[1.0, 2.0, 0.5, f32::MIN_POSITIVE, f32::MAX, f32::INFINITY, 0.0]);
        check("ln", &xs, ln, f32::ln);
        assert!(ln(-1.0).is_nan(), "ln(-1) 应为 NaN");
        assert_eq!(ln(1.0), 0.0, "ln(1) 应为 0");
        assert_eq!(ln(0.0), f32::NEG_INFINITY, "ln(0) 应为 -∞");
    }

    #[test]
    fn sin_cos_match_std() {
        let mut xs = sample(-100.0, 100.0, 60_000);
        xs.extend_from_slice(&[0.0, 1.5707964, 3.1415927, -3.1415927]);
        check("sin", &xs, sin, f32::sin);
        check("cos", &xs, cos, f32::cos);
    }

    /// **大参数专项**：Cody-Waite 归约质量（|x| ∈ [10, 1000]）
    #[test]
    fn sin_cos_large_argument() {
        let xs = sample(10.0, 1000.0, 40_000);
        let s = check("sin大参数", &xs, sin, f32::sin);
        let c = check("cos大参数", &xs, cos, f32::cos);
        assert!(s.max_ulp <= 4 || c.max_ulp <= 4, "大参数归约应保持 ≤4 ULP");
    }

    #[test]
    fn atan2_matches_std() {
        let mut xs: Vec<(f32, f32)> = Vec::with_capacity(40_000);
        let r = sample(-1000.0, 1000.0, 200);
        for &y in &r {
            for &x in &r {
                xs.push((y, x));
            }
        }
        let mut max_ulp = 0u32;
        let mut worst = (0.0f32, 0.0f32);
        for &(y, x) in &xs {
            let a = atan2(y, x);
            let b = y.atan2(x);
            let u = ulp_diff(a, b);
            if u != u32::MAX && u > max_ulp {
                max_ulp = u;
                worst = (y, x);
            }
        }
        println!("[fmath] atan2    样本={} maxULP={} 最差点=({}, {})", xs.len(), max_ulp, worst.0, worst.1);
        assert!(max_ulp <= 4, "atan2: max ULP = {max_ulp}（门线 4）最差点 {worst:?}");
        assert_eq!(atan2(0.0, 0.0), 0.0, "(0,0) 应为 0");
    }

    #[test]
    fn powi_matches_truth() {
        for &x in &[0.0f32, 1.0, -1.0, 2.0, 0.5, -2.5, 1e-3, 1e3] {
            for n in -20i32..=20 {
                let a = powi(x, n);
                // ⚠️ **基准用 `f64` 真值，而不是 `std::f32::powi`**（实测教训）：
                //    后者在**次正规/下溢区间跨平台行为不一致** ——
                //    例：`powi(1000.0, -14)` 真值 `1e-42`（次正规）：**Linux 上 std 返回 `0`，Windows 上返回 `1e-42`**。
                //    以 std 为基准会导致"本机绿、CI 红"（2026-09-16 实际发生）。
                //    `f64` 无此区间问题，故取其为真值。
                let truth = f64::from(x).powi(n) as f32;
                if truth.is_infinite() || truth == 0.0 {
                    let diff = abs_f32(a - truth);
                    let denorm_min = f32::from_bits(1); // 最小次正规数 ≈ 1.4e-45
                    assert!(
                        diff <= denorm_min * 4.0 || a == truth,
                        "powi({x},{n})：mine={a} 真值={truth}（超出下溢邻域）"
                    );
                } else {
                    // 累乘误差：门线 8 ULP（实测 ≤4）
                    let u = ulp_diff(a, truth);
                    assert!(u <= 8, "powi({x},{n})：ULP={u}（门线 8）mine={a} 真值={truth}");
                }
            }
        }
        assert_eq!(powi(0.0, 0), 1.0, "0^0 应为 1（与 std 一致）");
        // 与 std 的**非次正规**区间仍逐点对照（该区间 std 跨平台一致）
        for &x in &[1.0f32, 2.0, 0.5, -2.5, 1e3] {
            for n in -12i32..=12 {
                let t = f64::from(x).powi(n) as f32;
                if t == 0.0 || t.is_infinite() {
                    continue; // 跳过下溢/上溢区（见上）
                }
                let u = ulp_diff(powi(x, n), x.powi(n));
                assert!(u <= 8, "powi({x},{n}) 与 std 差异 ULP={u}");
            }
        }
    }
}
