//! # 1.2 双向通路（**纯算层**）：自上而下预测 ＋ 自下而上误差
//!
//! **本模块只做「算」**（机制 21）：给数值 → 算 → 回数值。不碰文件／时钟／外设 ⇒ 可进内核。
//!
//! ## 它在整个 2.4 里的位置
//! L0–L6 的「生成模型」落到实现上，最小可算的形态就是**预测编码**的两条通路：
//! - **自上而下（top-down）**：高层状态给出**预测** `prior`（先验）；
//! - **自下而上（bottom-up）**：观测 `obs` 与预测之差即**预测误差** `err`，
//!   误差**反向传播**去更新各层 `prior`。
//! ⇒ 与底图「大脑是预测机器」的口径一致（见 `docs/LAYER_BASEMAP_L0_L6.md`）。
//!
//! ## 为什么必须是纯算
//! _predict_ 只依赖「上一时刻的 prior + 本次观测」⇒ **无感知、无动作** ⇒ 落在内核侧（机制 21）；
//! 真正的「观测」来自宿主／边界层，**由调用方把数喂进来**。
//!
//! ## 守约
//! **C1**（零第三方依赖：只用 `core`）｜**C3/C9**（零 `unsafe`）｜**C5**（未验证的如实标注）。

use crate::math::clamp01;

/// 一层的预测状态。
///
/// - `prior`：该层对下一层的**预测值**（自上而下）；
/// - `precision`：**精度**（误差的权重；0 ⇒ 该层"不信"自己的观测 ⇒ 不更新）；
/// - `lr`：**学习率**（误差回灌到 prior 的比例，`[0,1]`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PredictLayer {
    /// 自上而下的预测值。
    pub prior: f32,
    /// 该层误差的精度权重（`[0,1]`，0 ⇒ 不更新）。
    pub precision: f32,
    /// 学习率（`[0,1]`）。
    pub lr: f32,
}

impl PredictLayer {
    /// 构造。**钳位**：`precision`、`lr` 一律夹到 `[0,1]`（越界即静默改正，
    /// 避免"负值学习率"把迭代推向发散）。
    #[must_use]
    pub fn new(prior: f32, precision: f32, lr: f32) -> Self {
        Self {
            prior,
            precision: clamp01(precision),
            lr: clamp01(lr),
        }
    }

    /// 自上而下：本层给出的**预测**。
    #[must_use]
    pub fn predict(&self) -> f32 {
        self.prior
    }

    /// 自下而上：给定观测，算**预测误差** `err = (obs − prior) × precision`。
    ///
    /// ★ **精度在这里起作用**：`precision = 0` ⇒ 误差恒为 0 ⇒ 该层**不接受**观测
    /// （对应"该通道噪声太大，暂不采信"）。
    #[must_use]
    pub fn error(&self, obs: f32) -> f32 {
        (obs - self.prior) * self.precision
    }

    /// 一步更新：算误差，并把它按 `lr` 回灌进 `prior`。返回**本次误差**。
    pub fn step(&mut self, obs: f32) -> f32 {
        let err = self.error(obs);
        self.prior = update_prior(self.prior, err, self.lr);
        err
    }
}

/// 预测误差（裸函数版，便于不用结构体的场合）。
#[must_use]
pub fn prediction_error(prior: f32, obs: f32, precision: f32) -> f32 {
    (obs - prior) * clamp01(precision)
}

/// 由误差更新预测：`prior' = prior + lr × err`，并**钳回 `[0,1]`**。
///
/// ⚠️ **为什么钳位**：本项目的状态量（能量／饱和度／势）定义在 `[0,1]`（见 `math.rs::clamp01`
/// 与底图「模糊饱和运算」）；不钳位会跑出 `[0,1]` 之外的"看起来更聪明"的预测，
/// 而下游一律按 `[0,1]` 解释 ⇒ **越界的预测是假预测**。
#[must_use]
pub fn update_prior(prior: f32, err: f32, lr: f32) -> f32 {
    clamp01(prior + clamp01(lr) * err)
}

/// **多层栈**：自上而下传播预测、自下而上传播误差。
///
/// 约定：`layers[0]` 是**最底层（贴观测）**，`layers[len-1]` 是**最高层**。
#[derive(Debug)]
pub struct PredictStack<'a> {
    /// 各层状态（**借用**，不持有存储 ⇒ 纯算层不分配）。
    pub layers: &'a mut [PredictLayer],
}

impl<'a> PredictStack<'a> {
    /// 包一层（空栈也允许，各方法按 0 处理）。
    #[must_use]
    pub fn new(layers: &'a mut [PredictLayer]) -> Self {
        Self { layers }
    }

    /// 层数。
    #[must_use]
    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    /// **自上而下**：把**最高层**的 `prior` 逐层向下灌（高层预测成为低层的先验），
    /// 返回**最底层**最终给出的预测（即"系统将看到什么"）。
    ///
    /// ⚠️ **口径**：这是**预测的下行**，不是"误差的下行"；它**不改变**任何层的 `prior`
    /// （改变 `prior` 是 `bottom_up` 的事）—— 两者**必须分开**（C19 概念分离：
    /// "预测传播"与"学习更新"是两件事，一个返回值只代表一件）。
    #[must_use]
    pub fn top_down(&self) -> f32 {
        if self.layers.is_empty() {
            return 0.0;
        }
        // 从最高层往下：每层 prior 被上一层"拉"一部分（这里取上层值本身作为下层先验）。
        let mut p = self.layers[self.layers.len() - 1].prior;
        for i in (0..self.layers.len() - 1).rev() {
            // 下层先验 = 上层传下来的预测（l = 该层学习率决定"听多少"）。
            let lr = self.layers[i].lr;
            p = clamp01(self.layers[i].prior + lr * (p - self.layers[i].prior));
        }
        p
    }

    /// **自下而上**：把观测的误差**自底层往上传**，逐层更新各层 `prior`。
    /// 返回**被更新的层数**。
    ///
    /// 做法：底层先算误差 ⇒ 更新 ⇒ 把**更新后的 prior** 当作上一层的"观测"，逐层上行。
    pub fn bottom_up(&mut self, obs: f32) -> usize {
        if self.layers.is_empty() {
            return 0;
        }
        let mut signal = obs;
        let mut n = 0usize;
        for i in 0..self.layers.len() {
            let err = self.layers[i].step(signal);
            signal = self.layers[i].prior;
            if err != 0.0 {
                n += 1;
            }
        }
        n
    }

    /// **自由能（简化版）**：`Σ err_i²`（各层误差的平方和）。
    ///
    /// 用途：作为"系统惊讶程度"的**纯算度量**——值越小 ⇒ 预测与观测越吻合。
    /// ⚠️ 这是**工程近似**，不是变分自由能的完整定义（缺精度加权的对数项）；
    /// 一旦引入真正的概率口径，**必须另立函数**，不得就地改本函数的含义（C19）。
    #[must_use]
    pub fn free_energy(&self, obs: f32) -> f32 {
        let mut signal = obs;
        let mut acc = 0.0f32;
        for l in self.layers.iter() {
            let e = l.error(signal);
            acc += e * e;
            signal = l.prior;
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ① 误差的**符号与量值**：观测高于预测 ⇒ 误差为正；`precision` 线性缩放。
    #[test]
    fn error_sign_and_scale() {
        let l = PredictLayer::new(0.2, 1.0, 0.5);
        assert!(l.error(0.6) > 0.0, "观测高于预测 ⇒ 误差应为正");
        assert!((l.error(0.6) - 0.4).abs() < 1e-6);
        let half = PredictLayer::new(0.2, 0.5, 0.5);
        assert!((half.error(0.6) - 0.2).abs() < 1e-6, "precision=0.5 ⇒ 误差减半");
    }

    /// ② **阴性对照**：`precision = 0` ⇒ 误差恒 0、`prior` **永不动**（该通道不被采信）。
    #[test]
    fn zero_precision_never_updates() {
        let mut l = PredictLayer::new(0.3, 0.0, 0.9);
        for _ in 0..100 {
            assert_eq!(l.step(1.0), 0.0);
        }
        assert_eq!(l.prior, 0.3, "precision=0 ⇒ prior 不得被观测拉动");
    }

    /// ③ **阳性对照（收敛）**：反复喂同一观测 ⇒ `prior` 单调逼近观测值。
    #[test]
    fn prior_converges_to_observation() {
        let mut l = PredictLayer::new(0.0, 1.0, 0.5);
        let obs = 0.8f32;
        let mut prev_gap = (obs - l.prior).abs();
        for _ in 0..64 {
            l.step(obs);
            let gap = (obs - l.prior).abs();
            assert!(gap <= prev_gap + 1e-6, "间隙应单调不增");
            prev_gap = gap;
        }
        assert!(prev_gap < 1e-3, "64 步后应收敛到观测值，实际间隙 {}", prev_gap);
    }

    /// ④ **钳位**：`prior` 恒在 `[0,1]`（不吃"越界的预测"）。
    #[test]
    fn prior_stays_in_unit_range() {
        let mut l = PredictLayer::new(0.9, 1.0, 1.0);
        for _ in 0..10 {
            l.step(5.0); // 远超上界
            assert!(l.prior <= 1.0 && l.prior >= 0.0);
        }
        assert_eq!(l.prior, 1.0);
    }

    /// ⑤ **自上而下 vs 自下而上 必须分开**（C19）：`top_down` **不得**改动任何 `prior`。
    #[test]
    fn top_down_does_not_mutate() {
        let mut layers = [
            PredictLayer::new(0.1, 1.0, 0.5),
            PredictLayer::new(0.5, 1.0, 0.5),
            PredictLayer::new(0.9, 1.0, 0.5),
        ];
        let before = layers.map(|l| l.prior);
        let stack = PredictStack::new(&mut layers);
        let p = stack.top_down();
        assert!(p > 0.0 && p <= 1.0);
        // ⚠️ `stack.layers` 是**切片**（`&mut [PredictLayer]`）⇒ 无 `.map()`（E0599 实测踩过）；
        //    改为逐元素取值比对（语义等价，且**不需分配**）。
        let after = [
            stack.layers[0].prior,
            stack.layers[1].prior,
            stack.layers[2].prior,
        ];
        assert_eq!(after, before, "top_down 不得改动 prior");
    }

    /// ⑥ **双向闭合**：上行更新后，下行预测应**更贴近**观测（这就是"双向通路"的意义）。
    #[test]
    fn two_way_reduces_free_energy() {
        let mut layers = [
            PredictLayer::new(0.0, 1.0, 0.5),
            PredictLayer::new(0.0, 1.0, 0.5),
        ];
        let obs = 0.7f32;
        let e0 = {
            let s = PredictStack::new(&mut layers);
            s.free_energy(obs)
        };
        for _ in 0..32 {
            let mut s = PredictStack::new(&mut layers);
            s.bottom_up(obs);
        }
        let e1 = {
            let s = PredictStack::new(&mut layers);
            s.free_energy(obs)
        };
        assert!(e1 < e0, "双向更新后自由能应下降：{} → {}", e0, e1);
    }

    /// ⑦ **空栈不炸**（0 层 ⇒ 预测 0、更新 0 层、自由能 0）。
    #[test]
    fn empty_stack_is_safe() {
        let mut v: [PredictLayer; 0] = [];
        let s = PredictStack::new(&mut v);
        assert_eq!(s.depth(), 0);
        assert_eq!(s.top_down(), 0.0);
        assert_eq!(s.free_energy(0.5), 0.0);
    }
}
