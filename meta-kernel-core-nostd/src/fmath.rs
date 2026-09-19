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
/// `|r| ≤ π/4 ≈ 0.785`；再用泰勒（`sin` 到 `r¹³`、`cos` 到 **`r¹⁴`**，截断误差 **< 1e-15**），
/// 最后按 `n mod 4` 组合象限、取符号。
///
/// ⚠️ **2026-09-19（§7.6 补丁稿 · 第 1 步）**：`cos` 原先止于 `r¹²`（截断误差 `≈3.9e-13`），
/// 已补 `r¹⁴/14!`；**大参数区（`|x| ≳ 1e3`）的归约误差未修**（需 Payne-Hanek，见
/// `sin_cos_f64_large_argument_report` 的实测数字，**该区段本轮不宣称达标**）。
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
    // cos(r) = 1 - r²/2 + r⁴/24 - r⁶/720 + r⁸/40320 - r¹⁰/3628800 + r¹²/479001600 - r¹⁴/87178291200
    // ⚠️ 2026-09-19（§7.6 补丁稿 · 第 1 步）：**补上 `r¹⁴/14!`（`14! = 87_178_291_200`）**。
    //    改前止于 `r¹²`，在 `|r| = π/4` 处截断误差 ≈ **3.89e-13 ≈ 3500 ULP**（本机实测）；
    //    改后首项截断降到 `r¹⁶/16! ≈ 1.0e-15`。
    //    ★ 关键：`n ≡ 1 (mod 4)` 时 `sin(x) = cos(r)` ⇒ **这一项同时决定 `sin` 与 `cos` 的精度**
    //      （故本补丁对 `sin` 同样有效，实测见 `sin_cos_f64_truncation_bound`）。
    let c = 1.0
        + r2 * (-0.5
            + r2 * (1.0 / 24.0
                + r2 * (-1.0 / 720.0
                    + r2 * (1.0 / 40_320.0
                        + r2 * (-1.0 / 3_628_800.0
                            + r2 * (1.0 / 479_001_600.0 + r2 * (-1.0 / 87_178_291_200.0)))))));

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

    // atan(t) = t - t³/3 + t⁵/5 - …
    // ⚠️ **项数由 10 提到 16**（2026-09-17 片3 修正）：|t| ≤ 0.268 ⇒ 10 项残差 ≈ 0.268²¹/21 ≈ **5.7e-14**，
    //    对 f32 足够，但 **对 f64 不够**（f64 需要 ≲1e-17）。16 项残差 ≈ 0.268³³/33 ≈ **1.6e-21** ✓
    let t2 = t * t;
    let mut term = t;
    let mut sum = t;
    let mut i = 1u32;
    while i <= 16 {
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

/// 四舍五入到最近整数（**半值远离零**，与 `std` 的 `f32::round` 同口径）。
///
/// **为什么需要**：`core` 不提供浮点数学 ⇒ 2.3b 迁移时 `.round()` 无处可去（片3+ 会遇到 4 处）。
pub fn round(x: f32) -> f32 {
    if !x.is_finite() {
        return x; // NaN/±∞ 原样返回（与 std 一致）
    }
    // |x| ≥ 2^23 时 f32 已是整数（尾数只有 23 位）⇒ 直接返回，**避免"加 0.5 再截断"的精度陷阱**。
    if x >= 8_388_608.0 || x <= -8_388_608.0 {
        return x;
    }
    let t = if x >= 0.0 { x + 0.5 } else { x - 0.5 };
    // x86 上 f32→i64 为**饱和**转换（Rust 1.45+ 语义）
    t as i64 as f32
}

/// 以 2 为底的对数（`log2(x) = ln(x) / ln 2`）。
///
/// **为什么需要**：同 `round` —— 片3+ 有 1 处 `.log2()`。
pub fn log2(x: f32) -> f32 {
    if x <= 0.0 {
        // 与 std 对齐：log2(0) = -∞；log2(负数) = NaN
        return if x == 0.0 { f32::NEG_INFINITY } else { f32::NAN };
    }
    if !x.is_finite() {
        return x; // +∞ → +∞；NaN 已在上面按 NaN 处理
    }
    ln(x) / core::f32::consts::LN_2
}

// ================== f64 侧实现（**2.3b 片3 发现的需求**）==================

// **为什么必须有 f64 侧**（编译探针实测，不是推测）：`core` 对 f64 **只给 `abs`/`max`/`min`**，
// `round`/`sqrt`/`log2`/`exp`/`powi`/`floor`/`rem_euclid` **一律没有**。
// 片3 的 `ontology`（f64 `.round()`/`.sqrt()`）与 `state`（f64 `.log2()`）在 no_std 下
// **无处可去** ⇒ 不补 f64 实现，片3 就不成立（**先决条件**，不是可选项）。
// 精度判据与 f32 同口径：**与 `std` 逐点对照的 ULP 上限**（见文末测试）。

/// f64 平方根：**位技巧初值 + 牛顿迭代**（与 f32 同法）。
#[must_use]
pub fn sqrt_f64(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 || x.is_infinite() {
        return x; // 保 ±0；+∞ → +∞
    }
    // 次正规数：先放大 2^54 进入正规区，结果再缩回 2^27（否则初值位技巧不成立）
    if x < f64::MIN_POSITIVE {
        return sqrt_f64(x * 1.801_439_850_948_198_4e16) / 1.342_177_28e8;
    }
    let mut y = f64::from_bits((x.to_bits() >> 1) + 0x1ff8_0000_0000_0000);
    let mut i = 0;
    while i < 5 {
        y = 0.5 * (y + x / y);
        i += 1;
    }
    y
}

/// `2^k`（整指数，**用位模式构造**，不走 `powf`）。
fn exp2i(k: i64) -> f64 {
    if k >= 1024 {
        return f64::INFINITY;
    }
    if k <= -1075 {
        return 0.0;
    }
    if k >= -1022 {
        return f64::from_bits(((k + 1023) as u64) << 52);
    }
    f64::from_bits(1u64 << (k + 1074)) // 次正规
}

/// f64 自然指数：**范围归约 + 泰勒级数**（`x = k·ln2 + r`，`|r| ≤ ln2/2`）。
#[must_use]
pub fn exp_f64(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    // **Cody-Waite 两段式 ln2**（2026-09-17 片3 修正）：
    // 单段写法 `r = x - k*LN_2` 在 |x| 大时**抵消**掉有效数字 ——
    // 实测 `exp(-700)` 误差 **244 ULP**（k·ln2 ≈ -700，相乘舍入 ~1e-13 直接变成 r 的绝对误差）。
    // 两段写法：`LN2_HI` 的低位为零 ⇒ `k*LN2_HI` **精确**，且与 `x` 同量级 ⇒ 该减法**精确**（Sterbenz），
    // 残余误差只来自 `LN2_LO` 项，量级 ~1e-27 ⇒ 实测回到 ≤2 ULP。
    const LN2_HI: f64 = 0.693_147_180_369_123_8;
    const LN2_LO: f64 = 1.908_214_929_270_587_7e-10;
    let k = round_f64(x * core::f64::consts::LOG2_E);
    let r = (x - k * LN2_HI) - k * LN2_LO;
    let mut term = 1.0f64;
    let mut sum = 1.0f64;
    let mut i = 1.0f64;
    while i <= 20.0 {
        term *= r / i;
        sum += term;
        i += 1.0;
    }
    sum * exp2i(k as i64)
}

/// f64 以 2 为底的对数：**指数/尾数分解 + atanh 级数**。
#[must_use]
pub fn log2_f64(x: f64) -> f64 {
    if x <= 0.0 {
        return if x == 0.0 { f64::NEG_INFINITY } else { f64::NAN };
    }
    if !x.is_finite() {
        return x;
    }
    let mut xv = x;
    let mut adj = 0.0f64;
    if xv < f64::MIN_POSITIVE {
        xv *= 1.801_439_850_948_198_4e16; // ×2^54
        adj = -54.0;
    }
    let bits = xv.to_bits();
    let e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    // log2(m) = (2/ln2)·(t + t³/3 + t⁵/5 + …)，t = (m−1)/(m+1) ∈ [0, 1/3]
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let mut term = t;
    let mut sum = 0.0f64;
    let mut k = 1.0f64;
    let mut i = 0;
    while i < 24 {
        sum += term / k;
        term *= t2;
        k += 2.0;
        i += 1;
    }
    adj + (e as f64) + sum * (2.0 / core::f64::consts::LN_2)
}

/// f64 自然对数（`ln(x) = log2(x)·ln2`）。
#[must_use]
pub fn ln_f64(x: f64) -> f64 {
    log2_f64(x) * core::f64::consts::LN_2
}

/// f64 两参数反正切（与 f32 版同法；f32 版内部本就全用 f64，故此处只是去掉 `as f32`）。
#[must_use]
pub fn atan2_f64(y: f64, x: f64) -> f64 {
    const PI: f64 = 3.141_592_653_589_793;
    const FRAC_PI_2: f64 = 1.570_796_326_794_896_6;
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x.is_infinite() || y.is_infinite() {
        let (xi, yi) = (x.is_infinite(), y.is_infinite());
        return match (xi, yi) {
            (true, false) => {
                if x > 0.0 {
                    if y > 0.0 { 0.0 } else if y < 0.0 { -0.0 } else { 0.0 }
                } else if y > 0.0 {
                    PI
                } else if y < 0.0 {
                    -PI
                } else {
                    PI
                }
            }
            (false, true) => {
                if y > 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 }
            }
            (true, true) => {
                let base = if x > 0.0 { PI / 4.0 } else { 3.0 * PI / 4.0 };
                if y > 0.0 { base } else { -base }
            }
            (false, false) => unreachable!(),
        };
    }
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let ax = abs_f64(x);
    let ay = abs_f64(y);
    let a = if ax >= ay { atan_f64(ay / ax) } else { FRAC_PI_2 - atan_f64(ax / ay) };
    if x >= 0.0 {
        if y >= 0.0 { a } else { -a }
    } else if y >= 0.0 {
        PI - a
    } else {
        a - PI
    }
}

/// f64 整数幂（平方-乘；`n < 0` 先取倒数——与 f32 版同一教训）。
#[must_use]
pub fn powi_f64(x: f64, n: i32) -> f64 {
    if n == 0 {
        return 1.0;
    }
    let (mut base, mut e): (f64, i64) = (x, n.unsigned_abs() as i64);
    let mut acc = 1.0f64;
    while e > 0 {
        if e & 1 == 1 {
            acc *= base;
        }
        base *= base;
        e >>= 1;
    }
    if n > 0 {
        return acc;
    }
    // ⚠️ **负数指数：先在"最后"取倒数**（2026-09-17 片3 修正）。
    //    初版照抄 f32 的"**先**取倒数再累乘"：那样每步都在放大误差 ——
    //    实测 `powi_f64(-2.5, -20)` 达 **15 ULP**；改为最后取倒数后为 ~1 ULP。
    //    但"最后取倒数"在**已上溢/下溢**时会把真值变成 0 或 ∞ ⇒ 退化时回退到"先取倒数"路径
    //    （该路径精度差，但**只在退化区**发生，且比返回错值好）。
    if acc.is_finite() && acc != 0.0 {
        return 1.0 / acc;
    }
    let mut b = 1.0 / x;
    let mut e2 = n.unsigned_abs() as i64;
    let mut a2 = 1.0f64;
    while e2 > 0 {
        if e2 & 1 == 1 {
            a2 *= b;
        }
        b *= b;
        e2 >>= 1;
    }
    a2
}

/// `rem_euclid`（**与 `std` 源码逐字同法**：`%` 后把负余数加 `|rhs|`）。
/// `core` 不提供它（编译探针实测）⇒ no_std 下 `.rem_euclid()` 需要这条替身。
#[must_use]
pub fn rem_euclid_f32(x: f32, rhs: f32) -> f32 {
    let r = x % rhs;
    if r < 0.0 { r + abs_f32(rhs) } else { r }
}

/// `f64` 版 `rem_euclid`（同法）。
#[must_use]
pub fn rem_euclid_f64(x: f64, rhs: f64) -> f64 {
    let r = x % rhs;
    if r < 0.0 { r + abs_f64(rhs) } else { r }
}

// ============================== `FloatOps`：让迁移"零改调用点" ==============================

/// **`no_std` 浮点运算替身**（`core` 不提供浮点数学，`std` 用不了）。
///
/// **为什么用 trait 而不是逐个改调用点**（2.3b 的关键技术前提）：
/// - `no_std` 下 `std` 不在场 ⇒ 内在方法不存在 ⇒ `x.abs()` **解析到本 trait**，**调用点一行不改**；
/// - host 的 `cargo test` 下**内在方法优先于 trait 方法** ⇒ 同一份源码**两边都能编**。
/// ⇒ 迁移动作从「151 处机械改写」降为「**加 1 行 `use`**」。
///
/// **⚠️ host 下本 trait 不会被调用** ⇒ `use crate::fmath::FloatOps;` 可能被判"未使用"
/// ⇒ 使用处须加 `#[allow(unused_imports)]`（**机制的必然结果**，不是回避警告）。
///
/// **⚠️ 为什么返回 `Self` 而不是 `f32`**（2026-09-17 片3 修正）：初版只服务 f32，
/// 于是片3 的 `ontology`（f64 `round`/`sqrt`）与 `state`（f64 `log2`）**无处可去**。
/// 改 `Self` 后**同一 trait 覆盖 f32/f64**，已迁文件的 `use` 行无需再改。
pub trait FloatOps: Copy {
    /// 绝对值（`std::f32::abs` / `std::f64::abs` 的替身）。
    fn abs(self) -> Self;
    /// 平方根。
    fn sqrt(self) -> Self;
    /// 自然指数。
    fn exp(self) -> Self;
    /// 自然对数。
    fn ln(self) -> Self;
    /// 正弦。
    fn sin(self) -> Self;
    /// 余弦。
    fn cos(self) -> Self;
    /// 反正切（`y.atan2(x)`）。
    fn atan2(self, other: Self) -> Self;
    /// 整数次幂。
    fn powi(self, n: i32) -> Self;
    /// 四舍五入。
    fn round(self) -> Self;
    /// 以 2 为底的对数。
    fn log2(self) -> Self;
    /// 欧几里得余数（**`core` 不提供**，片3 需要）。
    fn rem_euclid(self, rhs: Self) -> Self;
}

impl FloatOps for f32 {
    fn abs(self) -> f32 {
        abs_f32(self)
    }
    fn sqrt(self) -> f32 {
        sqrt(self)
    }
    fn exp(self) -> f32 {
        exp(self)
    }
    fn ln(self) -> f32 {
        ln(self)
    }
    fn sin(self) -> f32 {
        sin(self)
    }
    fn cos(self) -> f32 {
        cos(self)
    }
    fn atan2(self, other: f32) -> f32 {
        atan2(self, other)
    }
    fn powi(self, n: i32) -> f32 {
        powi(self, n)
    }
    fn round(self) -> f32 {
        round(self)
    }
    fn log2(self) -> f32 {
        log2(self)
    }
    fn rem_euclid(self, rhs: f32) -> f32 {
        rem_euclid_f32(self, rhs)
    }
}

impl FloatOps for f64 {
    fn abs(self) -> f64 {
        abs_f64(self)
    }
    fn sqrt(self) -> f64 {
        sqrt_f64(self)
    }
    fn exp(self) -> f64 {
        exp_f64(self)
    }
    fn ln(self) -> f64 {
        ln_f64(self)
    }
    fn sin(self) -> f64 {
        sin_cos_f64(self).0
    }
    fn cos(self) -> f64 {
        sin_cos_f64(self).1
    }
    fn atan2(self, other: f64) -> f64 {
        atan2_f64(self, other)
    }
    fn powi(self, n: i32) -> f64 {
        powi_f64(self, n)
    }
    fn round(self) -> f64 {
        round_f64(self)
    }
    fn log2(self) -> f64 {
        log2_f64(self)
    }
    fn rem_euclid(self, rhs: f64) -> f64 {
        rem_euclid_f64(self, rhs)
    }
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

    // ============ §7.6 · `f64` 精度审计（2026-09-19 夜间追加，补丁稿"改动 ③"） ============
    //
    // **为什么必须单独做 f64 级审计**：上面的 `sin_cos_match_std` 走的是 **`f32` 出口**，
    // 而 `f32` 的分辨率（`~6e-8`）**远大于** `f64` 级截断误差（`~4e-13`）
    // ⇒ **缺陷在 f32 出口上完全不可见**（本项目既有教训：**f32 分辨率会掩盖 f64 误差**）。
    // ⇒ 本组测试**直接测私有 `sin_cos_f64`**，与 `std` 的 `f64` 实现逐点对照。

    /// `f64` ULP 距离（**审计专用**）。
    ///
    /// 跨零 / NaN / ∞ ⇒ 返回 `u64::MAX`（＝"不适用"），由调用方跳过并改看绝对误差。
    /// ⚠️ **本函数是"第二份 ULP 实现"**（第一份是上面的 `ulp_diff(f32)`）——两处口径**不同源**
    /// （位宽不同、跨零处置不同）是可接受的，但**不得**把二者互相引用当作同一判据（C19 精神）。
    fn ulp_diff_f64(a: f64, b: f64) -> u64 {
        if a.is_nan() && b.is_nan() {
            return 0;
        }
        if a.is_infinite() || b.is_infinite() || a.is_nan() || b.is_nan() {
            return u64::MAX;
        }
        if (a < 0.0) != (b < 0.0) {
            return u64::MAX; // 跨零：ULP 无意义（改看 maxAbs）
        }
        (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
    }

    /// 一条测带上的审计结果。
    ///
    /// ⚠️ **两个"最差点"必须分开记**（否则会误读）：`worst_x` 是 **max ULP** 的点，
    /// `worst_abs_x` 是 **max 绝对误差**的点 —— 二者**通常不是同一个点**。
    /// 反例（本文件 2026-09-19 实测）：`sin` 在 `x = -2π` 处 `|sin|≈0` ⇒ **ULP 爆表**
    /// （`1.6e11`）而**绝对误差极小**；真正的绝对误差最大点落在 `|sin|≈1` 处。
    /// ⇒ **只看 ULP 会得出"sin 比 cos 差 4 个数量级"的错误结论**（R35：报数写口径）。
    struct F64Stat {
        max_ulp: u64,
        worst_x: f64,
        max_abs: f64,
        worst_abs_x: f64,
        n_skipped: usize,
        all_finite: bool,
    }

    /// 对 `sin_cos_f64`（**私有**）做 `f64` 级逐点对照。`which_cos = false` 测 `sin`。
    fn audit_f64(xs: &[f64], which_cos: bool) -> F64Stat {
        let mut st = F64Stat {
            max_ulp: 0,
            worst_x: 0.0,
            max_abs: 0.0,
            worst_abs_x: 0.0,
            n_skipped: 0,
            all_finite: true,
        };
        for &x in xs {
            let (ms, mc) = sin_cos_f64(x);
            let mine = if which_cos { mc } else { ms };
            let theirs = if which_cos { x.cos() } else { x.sin() };
            if !mine.is_finite() {
                st.all_finite = false;
            }
            let d = (mine - theirs).abs();
            if !(d <= st.max_abs) {
                // `!(d <= max)` 而非 `d > max`：让 NaN/inf **一定会覆盖** max（否则 inf 会被静默吞掉）
                st.max_abs = d;
                st.worst_abs_x = x;
            }
            let u = ulp_diff_f64(mine, theirs);
            if u == u64::MAX {
                st.n_skipped += 1;
            } else if u > st.max_ulp {
                st.max_ulp = u;
                st.worst_x = x;
            }
        }
        st
    }

    fn report_f64(name: &str, xs: &[f64], which_cos: bool) -> F64Stat {
        let st = audit_f64(xs, which_cos);
        println!(
            "[fmath·f64] {name:<20} 样本={:<6} maxULP={:<8} maxAbs={:.3e} ULP点={:<22} 绝对点={:<22} 跳过={} 全有限={}",
            xs.len(),
            st.max_ulp,
            st.max_abs,
            st.worst_x,
            st.worst_abs_x,
            st.n_skipped,
            st.all_finite
        );
        st
    }

    /// **§7.6 判据 1／2：`f64` 截断误差是否已降到"首项截断"量级。**
    ///
    /// **期望值不手写，从被测物本身的数学推导**（D40 精神）：
    ///   - `sin` 级数在 `r¹³` 截断（`sin(r) = r·Σ…+ r¹²/12!` 的最高次是 `r¹³/13!`）
    ///     ⇒ **首个被丢掉的项** = `r¹⁵/15!`；
    ///   - `cos` 级数原来在 `r¹²` 截断（`1/12! = 1/479_001_600`）
    ///     ⇒ **首个被丢掉的项** = `r¹⁴/14!`（**补丁要补的就是它**）。
    ///   - 归约后 `|r| ≤ π/4`（Cody-Waite 的经典界）⇒ 取 `rmax = π/4` 代入。
    /// **判据**：实测 `maxAbs` 应 **≤ 2 ×（首个被丢掉的项）** —— 系数 2 是给"Horner 求值自身的
    /// 舍入累积"留的余量（同量级，非自由调参）；若实测 **≫** 该界，说明**该补的项没补对**。
    ///
    /// ★ **象限交叉（本条是"差点写错期望值"的教训，故写进注释）**：`sin_cos_f64` 末尾按
    /// `n mod 4` 组合象限 —— **`q` 为奇数时 `sin(x) = ±cos(r)`、`cos(x) = ±sin(r)`（交叉）**。
    /// ⇒ **两个函数的误差界都必须取"两者较大者"**（即 `r¹⁵/15!`，来自 `sin` 级数），
    /// 而**不是**"`sin` 只受 `r¹⁵/15!` 约束、`cos` 只受 `r¹⁶/16!` 约束"。
    /// 初版按后者设界 ⇒ **实测判红**；核对象限表后确认**是期望值写错、不是补丁写错**
    /// （"断言失败先自查期望值"）⇒ 已改正。
    ///
    /// ★ **阳性对照**：**补丁施加前**本测**必须失败**（实测 `cos` 误差 `3.889e-13`，约为该界的 **95 倍**）；
    /// **施加后**通过（实测 `2.043e-14`，≈ `1.0 ×` 界）。⇒ 两张数字都要进报告（R35：报数写口径）。
    #[test]
    fn sin_cos_f64_truncation_bound() {
        let rmax = core::f64::consts::FRAC_PI_4; // π/4 ≈ 0.7853981633974483
        let mut p = 1.0f64;
        let mut last_omitted_sin = 0.0f64; // rmax^15 / 15!
        let mut last_omitted_cos = 0.0f64; // rmax^16 / 16!
        for k in 1..=16u32 {
            p *= rmax;
            if k == 15 {
                last_omitted_sin = p / factorial_f64(15);
            }
            if k == 16 {
                last_omitted_cos = p / factorial_f64(16);
            }
        }
        // 象限交叉 ⇒ 两个出口的误差界相同 = max(两项)
        let bound = 2.0 * last_omitted_sin.max(last_omitted_cos);
        println!(
            "[fmath·f64] 理论界：rmax=π/4 | 首个被丢掉项 sin(r¹⁵/15!)={:.4e} cos(r¹⁶/16!)={:.4e} | 统一界(2×max)={:.4e}",
            last_omitted_sin, last_omitted_cos, bound
        );

        // 主区间细扫（归约几乎无误差：|n| ≤ 4）＋ π/4 附近加密（截断误差最坏处）
        let mut xs: Vec<f64> = Vec::with_capacity(30_000);
        let two_pi = core::f64::consts::PI * 2.0;
        let n1 = 20_000;
        for i in 0..n1 {
            xs.push(-two_pi + two_pi * 2.0 * (i as f64 / (n1 - 1) as f64));
        }
        for k in -12..=12i32 {
            let base = k as f64 * core::f64::consts::FRAC_PI_2 + rmax;
            for j in -6..=6i32 {
                xs.push(base + (j as f64) * 1e-3);
            }
        }

        let ss = report_f64("sin 主区间 |x|≤2π", &xs, false);
        let cs = report_f64("cos 主区间 |x|≤2π", &xs, true);

        assert!(
            ss.max_abs <= bound,
            "f64 sin 截断误差 {:.4e} 超出界 {:.4e}（=2×max(r¹⁵/15!, r¹⁶/16!)）最差点 {}",
            ss.max_abs,
            bound,
            ss.worst_abs_x
        );
        assert!(
            cs.max_abs <= bound,
            "f64 cos 截断误差 {:.4e} 超出界 {:.4e}（=2×max(r¹⁵/15!, r¹⁶/16!)）最差点 {}",
            cs.max_abs,
            bound,
            cs.worst_abs_x
        );
    }

    fn factorial_f64(n: u32) -> f64 {
        let mut r = 1.0f64;
        let mut i = 2u32;
        while i <= n {
            r *= f64::from(i);
            i += 1;
        }
        r
    }

    /// **§7.6 观测带：`f64` 精度现状全景 —— 只登记、不设门线（大参数区）。**
    ///
    /// **为什么不给大参数区设门线**：`|x| ≥ 1e3` 的误差主因是 **`n = round(x·2/π)` 的舍入**
    /// （`x·2/π` 本身在 f64 下就舍入，`x` 越大 `n` 的**绝对**误差越大 ⇒ `r = x − n·π/2`
    /// **灾难性抵消**；Cody-Waite 三段只补偿 `π/2` 的表示误差，**补不了 `x·2/π` 的舍入**）。
    /// 治它需要 **Payne-Hanek 归约**（补丁稿"改动 ②"）；**本轮按指令只做第 1 步**。
    /// ⇒ 该区段**只输出数字**，**不得判绿**（否则就是"把未修的区段报成已修"）。
    ///
    /// ★ **本测顺带锁定一处新发现的真实缺陷**（见 `sin_cos_f64_huge_argument_known_defect`）：
    /// `|x| ≥ 1e100` 时 `r²` 溢出 ⇒ 返回 **`inf`/`NaN`**。
    #[test]
    fn sin_cos_f64_large_argument_report() {
        let mut xs: Vec<f64> = Vec::with_capacity(20_000);
        let n = 20_000;
        for i in 0..n {
            xs.push(10.0 + 990.0 * (i as f64 / (n - 1) as f64));
        }
        let a = report_f64("sin 中参数[10,1000]", &xs, false);
        let b = report_f64("cos 中参数[10,1000]", &xs, true);

        let huge: Vec<f64> = vec![
            1e3,
            1e6,
            1e9,
            1e12,
            1e15,
            1e18,
            1e30,
            1e100,
            1e300,
            -1e300,
            0.0,
            1.0e-300,
        ];
        report_f64("sin 大参数点", &huge, false);
        report_f64("cos 大参数点", &huge, true);

        // —— 只对**中参数区**断言，且只断言"数学上必然为真"的事实（不是门线）——
        for (st, nm) in [(&a, "sin[10,1000]"), (&b, "cos[10,1000]")] {
            assert!(st.all_finite, "{nm}: 结果必须全为有限值（`|x|≤1000` 时 `r²` 不会溢出）");
            assert!(st.max_abs <= 2.0, "{nm}: `|sin|/|cos| ≤ 1` ⇒ 误差必 ≤ 2，实测 {}", st.max_abs);
        }
    }

    /// **已知缺陷锁定（characterization test）**：`|x| ≥ 1e100` 时 `sin_cos_f64` 返回**非有限值**。
    ///
    /// **根因（本机实测推断）**：归约后 `r = x − n·π/2` 仍与 `x` **同量级**
    /// （因 `n·π/2` 与 `x` 的有效位几乎全部抵消，而 `x·2/π` 的舍入误差被放大到 `O(x)`）
    /// ⇒ `r²` 达 `1e160+`，泰勒多项式的 `r²⁷` 级**直接溢出 f64（`inf`）**。
    /// 同时 `n as i64` 对 `n > i64::MAX` 是**饱和转换** ⇒ `rem_euclid(4)` 的象限也失去意义。
    ///
    /// **本测的作用**：把"当前就是坏的"这一事实**钉在测试里**，使**将来任何修好它的改动
    /// 都会让本测失败**（＝提醒改判据），避免"修好了却没人发现判据还锁着旧行为"。
    /// **修它的路径**＝ Payne-Hanek 归约（补丁稿 §一 改动 ②，本轮未做）。
    #[test]
    fn sin_cos_f64_huge_argument_known_defect() {
        let huge: Vec<f64> = vec![1e100, 1e300, -1e300];
        let mut n_bad = 0usize;
        for &x in &huge {
            let (s, c) = sin_cos_f64(x);
            println!("[fmath·f64] 已知缺陷 |x|={:<8} sin={} cos={}", x, s, c);
            if !s.is_finite() || !c.is_finite() {
                n_bad += 1;
            }
        }
        assert_eq!(
            n_bad,
            huge.len(),
            "当前 `|x| ≥ 1e100` 应全部非有限（已知缺陷）；若此处失败，说明 Payne-Hanek 已被实现 ⇒ 请更新本判据"
        );
    }

    /// **大参数专项**：Cody-Waite 归约质量（|x| ∈ [10, 1000]）
    ///
    /// ⚠️ **2026-09-17（P3-3）修正**：原断言写的是 `s.max_ulp <= 4 || c.max_ulp <= 4` ——
    /// 用 **`||`** 意味着 **`sin` 与 `cos` 只要一侧达标就算通过**，**弱化了一半门线**（真实缺陷）。
    /// 实测两侧均为 **1 ULP**（`sin` 1／`cos` 1，40,000 点）⇒ 改为 **`&&`** 后**断言收紧且仍为绿**。
    #[test]
    fn sin_cos_large_argument() {
        let xs = sample(10.0, 1000.0, 40_000);
        let s = check("sin大参数", &xs, sin, f32::sin);
        let c = check("cos大参数", &xs, cos, f32::cos);
        assert!(
            s.max_ulp <= 4 && c.max_ulp <= 4,
            "大参数归约应保持 ≤4 ULP（sin={} cos={}）",
            s.max_ulp,
            c.max_ulp
        );
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

// ============================== `round` / `log2` 与 `FloatOps` 的检测 ==============================

#[cfg(test)]
mod floatops_tests {
    use super::*;

    /// 与 `std::f32::round` 逐点对照：**要么完全相等，要么同判 NaN**。
    #[test]
    fn round_matches_std() {
        let xs = [
            0.0f32, 0.4, 0.5, 0.6, 1.5, 2.5, -0.4, -0.5, -0.6, -1.5, -2.5, 3.14159, -3.14159,
            1e-7, -1e-7, 100.5, -100.5, 8_388_608.0, -8_388_608.0, 1.0e30, -1.0e30, f32::MIN,
            f32::MAX,
        ];
        for &x in &xs {
            let a = round(x);
            let b = x.round();
            assert!(
                (a.is_nan() && b.is_nan()) || a == b,
                "round({x}) 我们={a} std={b}"
            );
        }
        // 半值远离零（这是与"银行家舍入"的关键区别）
        assert_eq!(round(0.5), 1.0);
        assert_eq!(round(-0.5), -1.0);
        assert_eq!(round(2.5), 3.0);
    }

    /// `log2` 与 `std` 对照：取 ULP 判据（2.1 定的口径：≤ 4）。
    #[test]
    fn log2_matches_std() {
        let xs: [f32; 12] = [
            0.5, 1.0, 1.5, 2.0, 3.0, 10.0, 1024.0, 0.001, 1e6, 1e-6, 7.0, 12345.0,
        ];
        for &x in &xs {
            let a = log2(x);
            let b = x.log2();
            let ia = a.to_bits() as i64;
            let ib = b.to_bits() as i64;
            let u = (ia - ib).unsigned_abs() as u32;
            assert!(u <= 4, "log2({x}) 我们={a} std={b} ULP={u}");
        }
        // 边界
        assert_eq!(log2(2.0), 1.0);
        assert!(log2(0.0).is_infinite() && log2(0.0) < 0.0, "log2(0) 应为 -∞");
        assert!(log2(-1.0).is_nan(), "log2(负数) 应为 NaN");
    }

    /// **trait 可用性检测**：用 trait 的**显式调用**（`FloatOps::abs`）验证它真被实现。
    /// 用显式路径是为了**绕开"内在方法优先"** —— 否则这条断言会**空转**（在 host 上调到 std）。
    #[test]
    fn floatops_trait_is_implemented() {
        assert_eq!(FloatOps::abs(-3.5f32), 3.5);
        assert_eq!(FloatOps::round(2.5f32), 3.0);
        assert_eq!(FloatOps::log2(8.0f32), 3.0);
        assert_eq!(FloatOps::powi(2.0f32, 10), 1024.0);
        let s = FloatOps::sqrt(9.0f32);
        assert!((s - 3.0).abs() < 1e-6);
        let a = FloatOps::atan2(1.0f32, 1.0);
        assert!((a - core::f32::consts::FRAC_PI_4).abs() < 1e-6);
    }
}


// ============================== f64 与 rem_euclid 的精度测试 ==============================

#[cfg(test)]
mod f64_tests {
    use super::*;

    /// f64 的 ULP 距离（同号前提下有效；NaN/∞ 视为不适用）。
    fn ulp64(a: f64, b: f64) -> u64 {
        if a.is_nan() && b.is_nan() {
            return 0;
        }
        if a.is_infinite() || b.is_infinite() || a.is_nan() || b.is_nan() {
            return u64::MAX;
        }
        let ia = a.to_bits() as i64;
        let ib = b.to_bits() as i64;
        (ia - ib).unsigned_abs()
    }

    /// `sqrt_f64`：**≤1 ULP**（含次正规、极大/极小、完全平方点）。
    #[test]
    fn sqrt_f64_matches_std() {
        let xs: [f64; 18] = [
            1e-300, 1e-100, 1e-10, 0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 9.0, 100.0, 123456.789,
            1e10, 1e100, 1e300, f64::MIN_POSITIVE, 5e-324,
        ];
        let mut worst = 0u64;
        for &x in &xs {
            let a = sqrt_f64(x);
            let b = x.sqrt();
            let u = ulp64(a, b);
            worst = worst.max(u);
            assert!(u <= 1, "sqrt_f64({x:e}) 我们={a:e} std={b:e} ULP={u}");
        }
        // 完全平方数必须精确
        assert_eq!(sqrt_f64(4.0), 2.0);
        assert_eq!(sqrt_f64(9.0), 3.0);
        assert_eq!(sqrt_f64(0.0), 0.0);
        assert!(sqrt_f64(-1.0).is_nan(), "负数应为 NaN");
        assert_eq!(sqrt_f64(f64::INFINITY), f64::INFINITY);
        // 次正规输入必须能被处理（不返回 0 / 不返回 NaN）
        let s = sqrt_f64(f64::from_bits(1));
        assert!(s > 0.0 && s.is_finite(), "最小次正规开方应为正有限数，实得 {s:e}");
        println!("sqrt_f64 最差 ULP = {worst}");
    }

    /// `log2_f64`：**≤4 ULP**（与 f32 侧同口径），含 2 的幂精确点。
    #[test]
    fn log2_f64_matches_std() {
        let xs: [f64; 17] = [
            1e-300, 5e-324, 1e-100, 0.001, 0.5, 1.0, 1.5, 2.0, 3.0, 10.0, 1024.0, 12345.0, 1e6,
            1e-6, 1e100, 1e300, f64::MAX,
        ];
        let mut worst = 0u64;
        for &x in &xs {
            let a = log2_f64(x);
            let b = x.log2();
            let u = ulp64(a, b);
            worst = worst.max(u);
            assert!(u <= 4, "log2_f64({x:e}) 我们={a} std={b} ULP={u}");
        }
        // 精确点
        assert_eq!(log2_f64(1.0), 0.0);
        assert_eq!(log2_f64(2.0), 1.0);
        assert_eq!(log2_f64(1024.0), 10.0);
        assert_eq!(log2_f64(0.5), -1.0);
        // 边界与 std 对齐
        assert_eq!(log2_f64(0.0), f64::NEG_INFINITY);
        assert!(log2_f64(-1.0).is_nan());
        assert_eq!(log2_f64(f64::INFINITY), f64::INFINITY);
        println!("log2_f64 最差 ULP = {worst}");
    }

    /// `exp_f64` / `ln_f64`：**≤4 ULP**。
    #[test]
    fn exp_ln_f64_match_std() {
        let mut worst_e = 0u64;
        for &x in &[-700.0f64, -100.0, -1.0, -0.5, 0.0, 1e-15, 0.5, 1.0, 2.0, 10.0, 100.0, 700.0]
        {
            let a = exp_f64(x);
            let b = x.exp();
            let u = ulp64(a, b);
            worst_e = worst_e.max(u);
            assert!(u <= 4, "exp_f64({x}) 我们={a:e} std={b:e} ULP={u}");
        }
        assert_eq!(exp_f64(0.0), 1.0);
        let mut worst_l = 0u64;
        for &x in &[1e-300f64, 0.5, 1.0, 2.0, 10.0, 12345.0, 1e100, 1e300] {
            let a = ln_f64(x);
            let b = x.ln();
            let u = ulp64(a, b);
            worst_l = worst_l.max(u);
            assert!(u <= 4, "ln_f64({x:e}) 我们={a} std={b} ULP={u}");
        }
        assert_eq!(ln_f64(1.0), 0.0);
        println!("exp_f64 最差 ULP = {worst_e}｜ln_f64 最差 ULP = {worst_l}");
    }

    /// `powi_f64` / `atan2_f64`：**≤4 ULP**。
    #[test]
    fn powi_atan2_f64_match_std() {
        for &(x, n) in &[(2.0f64, 10i32), (1.5, -3), (-2.5, -20), (10.0, 15), (3.0, 0), (0.5, 40)]
        {
            let a = powi_f64(x, n);
            let b = x.powi(n);
            let u = ulp64(a, b);
            assert!(u <= 4, "powi_f64({x},{n}) 我们={a:e} std={b:e} ULP={u}");
        }
        for &(y, x) in &[(1.0f64, 1.0f64), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0), (3.0, 4.0), (0.0, 1.0)]
        {
            let a = atan2_f64(y, x);
            let b = y.atan2(x);
            let u = ulp64(a, b);
            assert!(u <= 4, "atan2_f64({y},{x}) 我们={a} std={b} ULP={u}");
        }
        assert_eq!(atan2_f64(0.0, 1.0), 0.0);
    }

    /// `round_f64`：**与 std 完全相等**（不是 ULP，是精确相等）。
    #[test]
    fn round_f64_exact() {
        for &x in &[
            0.0f64, 0.4, 0.5, 0.6, 1.5, 2.5, -0.4, -0.5, -0.6, -1.5, -2.5, 3.14159, -3.14159,
            1e-9, -1e-9, 1e30, -1e30, f64::MAX, f64::MIN,
        ] {
            assert_eq!(round_f64(x), x.round(), "round_f64({x})");
        }
        assert!(round_f64(f64::NAN).is_nan());
        assert_eq!(round_f64(f64::INFINITY), f64::INFINITY);
        assert_eq!(round_f64(0.5), 1.0);
        assert_eq!(round_f64(-0.5), -1.0);
        assert_eq!(round_f64(2.5), 3.0);
    }

    /// `rem_euclid`：**f32 与 f64 都要与 std 完全相等**（含负自变数、负除数、除零）。
    #[test]
    fn rem_euclid_matches_std() {
        for &(x, y) in &[
            (4.0f32, 2.0f32), (-4.0, 2.0), (1.0, -4.0), (-1.0, -4.0), (5.5, 2.0), (-5.5, 2.0),
            (0.0, 3.0), (3.0, 3.0), (-3.0, 3.0), (1e10, 7.0), (-1.0, 4.0),
        ] {
            assert_eq!(rem_euclid_f32(x, y), x.rem_euclid(y), "f32 rem_euclid({x},{y})");
        }
        for &(x, y) in &[
            (4.0f64, 2.0f64), (-4.0, 2.0), (1.0, -4.0), (-1.0, -4.0), (5.5, 2.0), (-5.5, 2.0),
            (3.0, 3.0), (-3.0, 3.0), (1e10, 7.0),
        ] {
            assert_eq!(rem_euclid_f64(x, y), x.rem_euclid(y), "f64 rem_euclid({x},{y})");
        }
        // 除零 ⇒ 双方都是 NaN
        assert!(rem_euclid_f32(1.0, 0.0).is_nan());
        assert!(rem_euclid_f64(1.0, 0.0).is_nan());
        // 典型语义（std 文档例）
        assert_eq!(rem_euclid_f32(-1.0, 4.0), 3.0);
    }

    /// `sin` / `cos` 的 **f64 路径**回归（**性质判据**；2026-09-17 P3-3 补缺）。
    ///
    /// ## ⚠️ 为什么本测试**不**断言 D29 的 ≤4 ULP 门线（**诚实标注**）
    ///
    /// 实测（host，基准 `std`，见报告 `2026-09-17_fmath精度回归补缺.md`）：
    ///
    /// | 量 | 实测 |
    /// |---|---|
    /// | `sin`/`cos` f64 **ULP 上界**（全域，含 `\|x\| ≤ 1000`） | **≈ 3500 ULP** |
    /// | **绝对误差上界** | **≈ 3.9e-13** |
    /// | 根因 | `cos` 泰勒级数**截断于 `r¹²`**（缺 `r¹⁴/14!`，该项残差 ≈ **3.87e-13**，与实测吻合）；`sin` 截断于 `r¹³` |
    /// | 门线（**D29**） | **≤ 4 ULP** ⇒ **未达标（差约 875×）** |
    ///
    /// 另有**归约失效区**（仅三段 Cody-Waite，无 Payne-Hanek）：
    /// `|x| ≈ 1e8` 起绝对误差 > 1e-9，`|x| ≥ 1e15` 起灾难性错误 —— **已在报告中登记为缺陷**。
    ///
    /// 按 **C5（诚实标注）／C7（不用 CI 通过代替真实运行）**：
    /// **既不降门线、也不打绿** ⇒ 本测试只固化**当前确实成立**的性质以拦截**静默回归**；
    /// **⚠️ 该缺陷修复后，应在此处补上「≤ 4 ULP」的门线断言。**
    #[test]
    fn sin_cos_f64_regression() {
        use FloatOps as F;

        // ① 特殊点：**位级精确**（零点 / 无穷 / NaN / 负零符号）
        assert_eq!(F::sin(0.0f64), 0.0);
        assert_eq!(F::cos(0.0f64), 1.0);
        assert_eq!(F::sin(-0.0f64).to_bits(), (-0.0f64).to_bits(), "sin(-0) 应保留负零");
        assert!(F::sin(f64::INFINITY).is_nan(), "sin(∞) 应为 NaN");
        assert!(F::cos(f64::NAN).is_nan(), "cos(NaN) 应为 NaN");

        // ② 奇偶性（**位级**）｜③ 周期 2π｜④ 恒等式 sin²+cos²=1｜⑤ 输出有界
        const N: usize = 20_001;
        let mut parity_sin = 0usize;
        let mut parity_cos = 0usize;
        let mut worst_id = 0.0f64;
        let mut worst_period = 0.0f64;
        for i in 0..N {
            let x = -1000.0 + 2000.0 * (i as f64) / ((N - 1) as f64);
            let (s, c) = (F::sin(x), F::cos(x));

            if F::sin(-x).to_bits() != (-s).to_bits() {
                parity_sin += 1;
            }
            if F::cos(-x).to_bits() != c.to_bits() {
                parity_cos += 1;
            }
            worst_id = worst_id.max((s * s + c * c - 1.0).abs());
            worst_period = worst_period.max((F::sin(x + core::f64::consts::TAU) - s).abs());

            assert!(s.is_finite() && c.is_finite(), "sin/cos 在 x={x:e} 返回非有限值");
            assert!(s.abs() <= 1.0 && c.abs() <= 1.0, "sin/cos 越界：x={x:e} s={s} c={c}");
        }
        assert_eq!(parity_sin, 0, "sin 奇对称性**位级**违例 {parity_sin} 处");
        assert_eq!(parity_cos, 0, "cos 偶对称性**位级**违例 {parity_cos} 处");
        // 门线取实测值的 2 个数量级余量（实测 5.77e-13 / 2.16e-14），**不是精度门线**
        assert!(worst_id <= 1e-12, "sin²+cos² 恒等式残差 {worst_id:e} 超过 1e-12");
        assert!(worst_period <= 1e-12, "周期 2π 残差 {worst_period:e} 超过 1e-12");

        // ⑥ 归约**有效域**（实测 |x| ≤ 1e7 时绝对误差 ≤ 1.2e-14；此处留 2 个数量级余量）
        //    ⚠️ 本断言**只覆盖 |x| ≤ 1e7** —— 更大参数的退化已在报告中登记为缺陷，**此处不固化错误行为**
        let mut worst_abs = 0.0f64;
        for &x in &[1.0f64, -1.0, 1e2, -1e2, 1e4, 1e6, 1e7, -1e7] {
            worst_abs = worst_abs.max((F::sin(x) - x.sin()).abs());
            worst_abs = worst_abs.max((F::cos(x) - x.cos()).abs());
        }
        assert!(
            worst_abs <= 1e-12,
            "|x| ≤ 1e7 的绝对误差 {worst_abs:e} 超过 1e-12（归约或级数可能被改坏）"
        );

        println!(
            "sin_cos_f64 回归：恒等式残差={worst_id:e}｜周期残差={worst_period:e}｜|x|≤1e7 绝对误差={worst_abs:e}\
｜（⚠️ 精度**未达** D29 的 ≤4 ULP 门线，本测试只固化性质）"
        );
    }

    /// **trait 对 f64 真被实现**（用**显式路径**调用，绕开 host 上的"内在方法优先"，否则本断言会空转）。
    #[test]
    fn floatops_trait_covers_f64() {
        assert_eq!(FloatOps::abs(-3.5f64), 3.5);
        assert_eq!(FloatOps::round(2.5f64), 3.0);
        assert_eq!(FloatOps::log2(8.0f64), 3.0);
        assert_eq!(FloatOps::sqrt(9.0f64), 3.0);
        assert_eq!(FloatOps::powi(2.0f64, 10), 1024.0);
        assert_eq!(FloatOps::rem_euclid(-1.0f64, 4.0), 3.0);
        assert_eq!(FloatOps::exp(0.0f64), 1.0);
        assert_eq!(FloatOps::ln(1.0f64), 0.0);
        assert!(FloatOps::sin(0.0f64).abs() < 1e-15);
        assert!((FloatOps::cos(0.0f64) - 1.0).abs() < 1e-15);
        assert!((FloatOps::atan2(1.0f64, 1.0) - core::f64::consts::FRAC_PI_4).abs() < 1e-15);
    }
}
