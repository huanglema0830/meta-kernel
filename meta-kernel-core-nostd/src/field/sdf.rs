//! # 场方程求解器 · **SDF（有符号距离场）**（纯算层子模块）
//!
//! 来源：`coordination/discussions/2026-09-19_场方程求解器设计稿.md` §2.2.4（纯算层 §3）
//! 与 §2.2.2(1)（主方程 `∇²V = ρ(SDF)` 的 `SDF` 项）。
//!
//! ## 口径（写死，勿猜）
//!
//! | 项 | 约定 |
//! |---|---|
//! | **符号** | **内负外正**（设计稿 §2.2.2(1) 表内明写）：`SDF < 0` ＝ 物体内部 |
//! | **零等值面** | 就是边界（`SDF = 0`） |
//! | **单位** | 与网格**同单位**（格距 = 1）。即"距离 2.5"表示"离边界 2.5 格" |
//! | **`f32`** | 本层全部用 `f32`（与求解器一致）；判据须按 `f32` 的 ULP 定 |
//!
//! ## 为什么 SDF 是"纯算"（机制 21 判据）
//!
//! 全部输入是**坐标与几何参数**，输出是**一个实数** —— **无感知、无动作、无 unsafe**。
//! ⇒ 可进 `nostd`、host 100% 单测（设计稿 §2.2.4 判据表首行）。
//!
//! ## 组合运算（并／交／差）
//!
//! 用 **`min` / `max`** 的硬组合（**精确距离**）。⚠️ **刻意不用 `smooth-min`**：
//! 光滑并会**改变距离语义**（不再是精确 SDF），属于"改变场" ⇒ 若要引入须先裁定
//! （与 §2.2.2(2) 选"软阈值 `ρ`"是两件事：那里平滑的是**源项**，这里平滑的是**几何**）。

// ⚠️ **`no_std` 纪律**：`f32::sqrt`／`f32::abs` **不在 `core` 的接口面内**（这正是 `fmath`
// 存在的原因）⇒ 本文件**一律**用 [`crate::fmath`] 的 `sqrt`／`abs_f32`，**不得**用固有方法。
use crate::fmath;

/// 圆（`(cx, cy)` 圆心，`r` 半径）。**内负外正**。
#[must_use]
pub fn circle(cx: f32, cy: f32, r: f32, x: f32, y: f32) -> f32 {
    let dx = x - cx;
    let dy = y - cy;
    // 用 `sqrt` 而非 `hypot`：`hypot` 不在 fmath 的接口面内（避免为此扩面）
    fmath::sqrt(dx * dx + dy * dy) - r
}

/// 轴对齐矩形（`(cx, cy)` 中心，`(hw, hh)` 半宽半高）。**内负外正**。
///
/// 解析式即 iq 的经典 `sdBox`：把"框外距离"与"框内距离"分两段。
#[must_use]
pub fn rect(cx: f32, cy: f32, hw: f32, hh: f32, x: f32, y: f32) -> f32 {
    let dx = fmath::abs_f32(x - cx) - hw;
    let dy = fmath::abs_f32(y - cy) - hh;
    let out = fmath::sqrt(dx.max(0.0) * dx.max(0.0) + dy.max(0.0) * dy.max(0.0));
    let ins = dx.max(dy).min(0.0);
    out + ins
}

/// 线段 `(ax, ay)–(bx, by)`（**无厚度**）。**内负外正**（线段上为 0）。
#[must_use]
pub fn segment(ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) -> f32 {
    let pax = x - ax;
    let pay = y - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = bax * bax + bay * bay;
    let t = if denom <= 0.0 {
        0.0
    } else {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    };
    let cx = pax - bax * t;
    let cy = pay - bay * t;
    fmath::sqrt(cx * cx + cy * cy)
}

/// 并集：`min`。
#[must_use]
pub fn union(a: f32, b: f32) -> f32 {
    a.min(b)
}

/// 交集：`max`。
#[must_use]
pub fn intersect(a: f32, b: f32) -> f32 {
    a.max(b)
}

/// 差集 `a \ b`：`max(a, -b)`。
#[must_use]
pub fn subtract(a: f32, b: f32) -> f32 {
    a.max(-b)
}

/// 可组合的几何体（**无 `alloc`、无 `Box`**：需要"组合"时用 [`Scene`] 的定长列表）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    /// 圆
    Circle { cx: f32, cy: f32, r: f32 },
    /// 轴对齐矩形
    Rect { cx: f32, cy: f32, hw: f32, hh: f32 },
    /// 线段
    Segment { ax: f32, ay: f32, bx: f32, by: f32 },
}

impl Shape {
    /// 取该几何体在 `(x, y)` 处的有符号距离（内负外正）。
    #[must_use]
    pub fn distance(&self, x: f32, y: f32) -> f32 {
        match *self {
            Shape::Circle { cx, cy, r } => circle(cx, cy, r, x, y),
            Shape::Rect { cx, cy, hw, hh } => rect(cx, cy, hw, hh, x, y),
            Shape::Segment { ax, ay, bx, by } => segment(ax, ay, bx, by, x, y),
        }
    }
}

/// 组合算子（与 [`union`]／[`intersect`]／[`subtract`] 一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// 并
    Union,
    /// 交
    Intersect,
    /// 差（`左 \ 右`）
    Subtract,
}

impl Op {
    #[must_use]
    pub fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            Op::Union => union(a, b),
            Op::Intersect => intersect(a, b),
            Op::Subtract => subtract(a, b),
        }
    }
}

/// **定长场景**（`N` 个几何体 ＋ `N` 个组合算子）：`SDF = seed ⊕₁ s₁ ⊕₂ s₂ ⊕₃ …`。
///
/// **为什么定长**：`nostd` 下不用 `Vec`／`Box` ⇒ 尺寸编译期已知、零分配、零 `unsafe`。
/// `N` 由调用方按场景规模选（`Scene::<8>::new()`）；算子是"从左到右依次应用"。
#[derive(Clone, Copy, Debug)]
pub struct Scene<const N: usize> {
    seed: Option<f32>,
    shapes: [Option<Shape>; N],
    ops: [Op; N],
    n: usize,
}

impl<const N: usize> Default for Scene<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Scene<N> {
    /// 空场景（还没有任何几何体 ⇒ 距离恒为 [`f32::INFINITY`]，表示"处处在界外"）。
    #[must_use]
    pub const fn new() -> Self {
        Self { seed: None, shapes: [None; N], ops: [Op::Union; N], n: 0 }
    }

    /// 追加一个几何体（超过 `N` 个则**忽略并返回 `false`**，不 panic）。
    pub fn push(&mut self, op: Op, s: Shape) -> bool {
        if self.n >= N {
            return false;
        }
        self.ops[self.n] = op;
        self.shapes[self.n] = Some(s);
        self.n += 1;
        true
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.n
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 求值：从左到右依次应用算子。
    #[must_use]
    pub fn distance(&self, x: f32, y: f32) -> f32 {
        let mut acc = self.seed.unwrap_or(f32::INFINITY);
        let mut i = 0;
        while i < self.n {
            if let Some(s) = self.shapes[i] {
                acc = self.ops[i].apply(acc, s.distance(x, y));
            }
            i += 1;
        }
        acc
    }

    /// 把整幅 `w × h` 网格采样进 `out`（`out.len()` 不足 ⇒ 返回已写点数；**不 panic**）。
    pub fn sample_into(&self, w: usize, h: usize, out: &mut [f32]) -> usize {
        let mut n = 0usize;
        let mut y = 0usize;
        while y < h {
            let mut x = 0usize;
            while x < w {
                if n >= out.len() {
                    return n;
                }
                out[n] = self.distance(x as f32, y as f32);
                n += 1;
                x += 1;
            }
            y += 1;
        }
        n
    }
}

/// 函数式采样（给"不想建 `Scene`"的调用方）：`f(x, y) → SDF` 采样进 `out`。
pub fn sample_into(f: impl Fn(f32, f32) -> f32, w: usize, h: usize, out: &mut [f32]) -> usize {
    let mut n = 0usize;
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        while x < w {
            if n >= out.len() {
                return n;
            }
            out[n] = f(x as f32, y as f32);
            n += 1;
            x += 1;
        }
        y += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 符号口径：**内负外正**（写死的约定，必须钉住）。
    #[test]
    fn sign_convention_inside_negative_outside_positive() {
        // 圆心 (5,5) 半径 2
        assert!(circle(5.0, 5.0, 2.0, 5.0, 5.0) < 0.0, "圆心必须在内部（负）");
        assert!((circle(5.0, 5.0, 2.0, 7.0, 5.0) - 0.0).abs() < 1e-6, "边界上必须 = 0");
        assert!((circle(5.0, 5.0, 2.0, 9.0, 5.0) - 2.0).abs() < 1e-6, "外部距离 = 2");
        assert!((circle(5.0, 5.0, 2.0, 5.0, 5.0) + 2.0).abs() < 1e-6, "圆心处 = -r");
    }

    /// 精确性：**距离值必须真等于欧氏距离**（不是近似）。
    ///
    /// 用 "3-4-5" 直角三角形做**手算可验**的对照：圆心 (0,0)，点 (3,4) ⇒ 距离 5。
    #[test]
    fn distances_are_exact_on_pythagorean_points() {
        assert!((circle(0.0, 0.0, 0.0, 3.0, 4.0) - 5.0).abs() < 1e-5);
        // 线段：竖直段 (0,0)-(0,10)，点 (3,5) ⇒ 距离 3
        assert!((segment(0.0, 0.0, 0.0, 10.0, 3.0, 5.0) - 3.0).abs() < 1e-5);
        // 线段端点外：点 (-3,-4) 到段 (0,0)-(0,10) ⇒ 到端点 (0,0) 的距离 = 5
        assert!((segment(0.0, 0.0, 0.0, 10.0, -3.0, -4.0) - 5.0).abs() < 1e-5);
        // 退化线段（a == b）⇒ 退化为点距离，不除零
        assert!((segment(1.0, 1.0, 1.0, 1.0, 1.0, 4.0) - 3.0).abs() < 1e-5);
    }

    /// 矩形：内外两段都要对（iq 的 `sdBox` 有两处分段，容易只对一半）。
    #[test]
    fn rect_inside_and_outside_branches() {
        let (cx, cy, hw, hh) = (0.0f32, 0.0f32, 2.0f32, 1.0f32);
        // 内部：距离 = -min(到各边距离) = -1（到上下边）… 到左右边为 2 ⇒ 取最小 ⇒ -1
        assert!((rect(cx, cy, hw, hh, 0.0, 0.0) + 1.0).abs() < 1e-5, "中心处 = -min(hw,hh)");
        // 正右方外部：dx = 5-2 = 3, dy = 0-1 = -1 ⇒ out=3, ins=min(-1,0)=-1? 应为 3
        assert!((rect(cx, cy, hw, hh, 5.0, 0.0) - 3.0).abs() < 1e-5, "正右外部 = 3");
        // 对角外部：(5, 4) ⇒ dx=3, dy=3 ⇒ √18 ≈ 4.2426
        assert!((rect(cx, cy, hw, hh, 5.0, 4.0) - fmath::sqrt(18.0)).abs() < 1e-4, "对角 = √18");
        // 边界
        assert!(rect(cx, cy, hw, hh, 2.0, 0.0).abs() < 1e-6);
    }

    /// 组合算子：并／交／差 的语义（用同一圆与矩形，可手算）。
    #[test]
    fn set_operations_semantics() {
        // 两个圆：左 (-2,0) 与右 (2,0)，半径都 1
        let l = |x, y| circle(-2.0, 0.0, 1.0, x, y);
        let r = |x, y| circle(2.0, 0.0, 1.0, x, y);
        // 并：中点 (0,0) 应在**外部两圆的并之外**？(0,0) 到两圆心距离都 2 ⇒ 并 = 2-1 = 1 > 0
        assert!((union(l(0.0, 0.0), r(0.0, 0.0)) - 1.0).abs() < 1e-5);
        // 交：两圆不相交 ⇒ 交的距离 > 0 且 ≥ max
        let inter = intersect(l(0.0, 0.0), r(0.0, 0.0));
        assert!((inter - 1.0).abs() < 1e-5);
        // 差：左圆减右圆 ⇒ 在左圆内部靠右处 (0,0) 距右圆 1 ⇒ max(-1, -1)? 左侧圆 (-2,0) 到 (0,0) 距离 2 ⇒ 2-1 = 1 >0
        assert!((subtract(l(0.0, 0.0), r(0.0, 0.0)) - 1.0).abs() < 1e-5);
        // 差的可判别情形：左圆内一点 (-2,0)：l = -1，r = 3 ⇒ subtract = max(-1, -3) = -1（仍在差集内）
        assert!((subtract(l(-2.0, 0.0), r(-2.0, 0.0)) + 1.0).abs() < 1e-5);
        // 而在右圆内 (2,0)：l = 3, r = -1 ⇒ subtract = max(3, 1) = 3（已被挖掉）
        assert!((subtract(l(2.0, 0.0), r(2.0, 0.0)) - 3.0).abs() < 1e-5);
        // Op 包装与自由函数必须同源（同值）
        assert_eq!(Op::Union.apply(1.5, -2.0), union(1.5, -2.0));
        assert_eq!(Op::Intersect.apply(1.5, -2.0), intersect(1.5, -2.0));
        assert_eq!(Op::Subtract.apply(1.5, -2.0), subtract(1.5, -2.0));
    }

    /// `Scene`：定长、无分配；从左到右应用；超容量不 panic；空场景为 +∞。
    #[test]
    fn scene_composes_and_never_panics() {
        let mut sc = Scene::<3>::new();
        assert!(sc.is_empty());
        assert_eq!(sc.distance(0.0, 0.0), f32::INFINITY, "空场景 ⇒ 处处在界外");
        assert!(sc.push(Op::Union, Shape::Rect { cx: 0.0, cy: 0.0, hw: 2.0, hh: 1.0 }));
        assert!(sc.push(Op::Subtract, Shape::Circle { cx: 0.0, cy: 0.0, r: 0.5 }));
        // ⚠️ 第三个几何体**刻意放在远处**（20,20）：若图省事写成"过原点的线段"，
        //    它会把被挖掉的中心**又加回来**（并集）⇒ 判据就失去判别力（这是我初版的错）。
        assert!(sc.push(Op::Union, Shape::Circle { cx: 20.0, cy: 20.0, r: 1.0 }));
        assert_eq!(sc.len(), 3);
        assert!(!sc.push(Op::Union, Shape::Circle { cx: 9.0, cy: 9.0, r: 1.0 }), "超容量 ⇒ false");
        // 中心：矩形给 -1，圆形减去 ⇒ max(-1, +0.5) = +0.5 ⇒ 在界外
        assert!(
            (sc.distance(0.0, 0.0) - 0.5).abs() < 1.0e-5,
            "中心应被圆形挖去：距离 = +0.5，实测 {}",
            sc.distance(0.0, 0.0)
        );
        // 与手工依次应用一致
        let expect = Op::Union.apply(
            Op::Subtract.apply(
                Op::Union.apply(
                    f32::INFINITY,
                    Shape::Rect { cx: 0.0, cy: 0.0, hw: 2.0, hh: 1.0 }.distance(1.0, 0.0),
                ),
                Shape::Circle { cx: 0.0, cy: 0.0, r: 0.5 }.distance(1.0, 0.0),
            ),
            Shape::Circle { cx: 20.0, cy: 20.0, r: 1.0 }.distance(1.0, 0.0),
        );
        assert_eq!(sc.distance(1.0, 0.0), expect, "Scene 与手算必须同值（同源）");
        // 采样：短缓冲不 panic
        let mut buf = [0.0f32; 5];
        assert_eq!(sc.sample_into(4, 4, &mut buf), 5);
    }

    #[test]
    fn functional_sampling_matches_scene() {
        let sc = {
            let mut s = Scene::<1>::new();
            s.push(Op::Union, Shape::Circle { cx: 3.0, cy: 3.0, r: 2.0 });
            s
        };
        let mut a = [0.0f32; 16];
        let mut b = [0.0f32; 16];
        assert_eq!(sc.sample_into(4, 4, &mut a), 16);
        let n = sample_into(|x, y| circle(3.0, 3.0, 2.0, x, y), 4, 4, &mut b);
        assert_eq!(n, 16);
        assert_eq!(a, b, "两条采样路径必须逐值一致（否则就是两份实现）");
    }
}
