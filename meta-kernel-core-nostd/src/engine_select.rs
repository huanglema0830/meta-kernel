//! # 物态 → 引擎 选择（Q11 可测契约 · 2026-09-19 裁定落地）
//!
//! **依据**：`docs/MATH_SPEC.md` **§5**（Q8／Q10 已裁定）＋
//! `coordination/discussions/2026-09-18_Q11可测契约稿_引擎选择表.md`（**U1／U2／U3 已裁定**）。
//!
//! **裁定摘要（逐条落地）**：
//! - **Q10 前问**：物态 ＝ **选择器**（**不是调制量**）⇒ **不引入映射 `g`**，引擎输入 ＝ 原种子（A1 原样）。
//! - **Q10 映射**：**选丙**（阈值处不切换）⇒ **除"引擎切换"外，阈值处无额外数值跳变**。
//! - **U1**（物态取自哪一层）：**选乙** ＝ [`state_of_energy_budget`]（**比值态 ⊕ 预算态，取更固者**）。
//! - **U2**（输出形态）：**选丙** ＝ **标签面 ＋ 结果面各出一个可测面**（见 [`select_engine`] ／ [`step_with`]）。
//! - **U3**（作用域）：`state_pace` 调制的是**调度层**，**不调制引擎输入** —— 与 Q10 前问**不冲突**
//!   （作用域限定已写入 `MATH_SPEC §5.2.0`）。
//!
//! **边界（本模块的纪律）**：
//! - **只调用**既有 API（`state`／`linear`／`fib`／`expo`），**不改动**它们（**C3**）；
//! - **零第三方依赖**（**C1**）；**无 `unsafe`**（**C9**）。
//!
//! **选择表（依 Q8 裁定 ＋ `GENE_LIBRARY_DESIGN §2.1 行61` 的四档）**：
//!
//! | 物态 | 引擎 | 说明 |
//! |---|---|---|
//! | 固态 | [`Engine::Linear`] | `入/出 < 0.8` |
//! | 液态 | [`Engine::Fibonacci`] | `0.8 ≤ 入/出 ≤ 1.05` |
//! | 气态 | [`Engine::Fibonacci`] | `1.05 < 入/出 ≤ 1.2` |
//! | 能量态 | [`Engine::Expo`] | `入/出 > 1.2` |
//!
//! ⇒ **液态与气态共用斐波那契** ⇒ **`1.05` 处「物态切换、引擎不切换」**，
//! **实际只有 2 个引擎切换点（`0.8` 与 `1.2`）**，不是 3 个。

//! 【2.3b 片9 · 补迁（R79 修复）】与 `meta-kernel-core/src/engine_select.rs` **同源**；
//! 仅作下述适配，其余**逐行逐字未改**：
//! ① **零 `alloc` 需求**（本文件无 `Vec`／`String`／`format!`／`ToString`）⇒ **无需补 `use`**
//! ② **无 `std::` 路径需改写**（本文件零 `std::` 用法）
//! ③ **零「替换类」**：无集合、无浮点方法、无 `std::` 常量 ⇒ 无类型/文字替换
//! ④ **补迁来由**：`engine_select` 新增于 v0.212，**未随 2.3b 分片迁入** ⇒
//!    `closure`（封闭性）判 PASS、**机制 25 报「未收口（剩余 1）」** ⇒ 二者口径不一致（**R79**）。
//!    补迁后 **未迁 = 0（源 ⊆ 目标）**，两判据口径恢复一致。
//! ⑤ 清单**由 `--emit` 产出**（D40），**不手写**。
use crate::energy::EnergyPool;
use crate::expo::ExpoEngine;
use crate::fib::FibEngine;
use crate::linear::LinearEngine;
use crate::state::{state_of_energy_budget, State};

/// 三引擎标识（**标签面** · U2 裁定＝丙 之"标签"）。
///
/// 与 `docs/MATH_SPEC.md §1` 的三引擎一一对应：线性／斐波那契／指数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// 线性引擎（**固态**）。
    Linear,
    /// 斐波那契引擎（**液态／气态**）。
    Fibonacci,
    /// 指数引擎（**能量态**）。
    Expo,
}

/// **物态 → 引擎**（Q8 裁定；**选择器**语义 —— 只决定"用哪个引擎"，不改输入值）。
///
/// 该函数**是纯映射**：无状态、无浮点运算 ⇒ 期望值**唯一可判**。
#[inline]
pub const fn engine_for_state(s: State) -> Engine {
    match s {
        State::Solid => Engine::Linear,
        State::Liquid | State::Gas => Engine::Fibonacci,
        State::Energy => Engine::Expo,
    }
}

/// **契约入口（标签面）** —— U1 裁定＝**乙**：物态取 [`state_of_energy_budget`]
/// （**比值态 ⊕ 预算态，取更固者**）。
///
/// ⇒ **储备枯竭会主动把物态封顶在更低能量级**（拉向固态），即使瞬时流入比值偏高；
/// 因此本函数的输入是**整个 [`EnergyPool`]**（比值 ＋ 储备），**不只 `r`**。
pub fn select_engine(pool: &EnergyPool) -> Engine {
    engine_for_state(state_of_energy_budget(pool))
}

/// **结果面**（U2 裁定＝丙 之"结果"）：按选中的引擎对种子 `x` 推**一步**。
///
/// - 每次调用**新建引擎**（不跨调用累积状态）—— 这是"**可测面**"的要求：
///   同一 `(engine, x)` 必得同一输出；
/// - **不改变**各引擎既有语义（`linear` 的 `sat_add`／`fib` 的点燃／`expo` 的护栏均按原样）。
pub fn step_with(engine: Engine, x: f32) -> f32 {
    match engine {
        Engine::Linear => LinearEngine::new().step(x),
        Engine::Fibonacci => FibEngine::new().step(x),
        Engine::Expo => ExpoEngine::default().step(x),
    }
}

/// **契约组合**（标签 ＋ 结果一次给出）：`(用哪个引擎, 该引擎对该种子的输出)`。
pub fn select_and_step(pool: &EnergyPool, x: f32) -> (Engine, f32) {
    let e = select_engine(pool);
    (e, step_with(e, x))
}

// =====================================================================
// Q11 可测契约 · 判据 D1–D7（**全部实跑**）
// =====================================================================
#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::state::state_of_flow_ratio;

    /// **ε 口径（R35）**：f32 在 `1.05` 附近的 ULP ≈ `1.19e-7`；本契约取 **`1e-6`**
    /// （可表示、且显著大于 ULP ⇒ 不会因舍入落到同一侧）。
    const EPS: f32 = 1e-6;

    /// 造池助手：`flow_out = 0` ⇒ `ratio()` 落到上界 `9.0`（"入远大于出"）。
    fn pool_high_ratio(stored: f32) -> EnergyPool {
        EnergyPool { flow_in: 1.0, flow_out: 0.0, stored }
    }

    // ---------------- D1 区间中点 ----------------
    #[test]
    fn d1_interval_midpoints() {
        let cases = [
            (0.4_f32, Engine::Linear),
            (0.9, Engine::Fibonacci),
            (1.1, Engine::Fibonacci),
            (1.5, Engine::Expo),
        ];
        for (r, want) in cases {
            let got = engine_for_state(state_of_flow_ratio(r));
            assert_eq!(got, want, "D1: r={} 应选 {:?}", r, want);
        }
    }

    // ---------------- D2 边界点（严格不等式）----------------
    #[test]
    fn d2_boundary_points() {
        // (r, 期望物态, 期望引擎)
        let cases = [
            (0.8_f32, State::Liquid, Engine::Fibonacci), // ≥0.8 ⇒ 液态
            (0.7999999, State::Solid, Engine::Linear),   // <0.8 ⇒ 固态
            (1.05, State::Liquid, Engine::Fibonacci),    // >1.05 才是气态 ⇒ 1.05 本身是液态
            (1.05 + EPS, State::Gas, Engine::Fibonacci), // 物态变、引擎不变
            (1.2, State::Gas, Engine::Fibonacci),        // >1.2 才是能量态 ⇒ 1.2 本身是气态
            (1.2 + EPS, State::Energy, Engine::Expo),    // 切换点
        ];
        for (r, want_state, want_eng) in cases {
            let s = state_of_flow_ratio(r);
            assert_eq!(s, want_state, "D2: r={} 物态应为 {:?}", r, want_state);
            assert_eq!(engine_for_state(s), want_eng, "D2: r={} 引擎应为 {:?}", r, want_eng);
        }
    }

    // ---------------- D3 不切换性（Q10 丙案直接推论）----------------
    #[test]
    fn d3_no_engine_switch_at_1_05() {
        let lo = engine_for_state(state_of_flow_ratio(1.05 - EPS));
        let hi = engine_for_state(state_of_flow_ratio(1.05 + EPS));
        assert_eq!(lo, hi, "D3: 1.05 两侧必须同引擎（丙案）");
        assert_eq!(lo, Engine::Fibonacci, "D3: 两侧同为斐波那契");
        // 对照：0.8 与 1.2 才是真切换点
        assert_ne!(
            engine_for_state(state_of_flow_ratio(0.8 - EPS)),
            engine_for_state(state_of_flow_ratio(0.8 + EPS)),
            "D3: 0.8 处必须换引擎"
        );
        assert_ne!(
            engine_for_state(state_of_flow_ratio(1.2 - EPS)),
            engine_for_state(state_of_flow_ratio(1.2 + EPS)),
            "D3: 1.2 处必须换引擎"
        );
    }

    // ---------------- D4 不调制性（Q10 丙案直接推论）----------------
    #[test]
    fn d4_no_modulation_within_same_state() {
        let x = 0.4_f32;
        // 液态区间内的三个 r（0.9 / 1.0 / 1.05）⇒ 同引擎、**同输出**
        let outs = [0.9_f32, 1.0, 1.05].map(|r| {
            let e = engine_for_state(state_of_flow_ratio(r));
            (e, step_with(e, x))
        });
        assert_eq!(outs[0].0, outs[1].0, "D4: 同物态应同引擎");
        assert_eq!(outs[1].0, outs[2].0, "D4: 同物态应同引擎");
        assert_eq!(outs[0].1, outs[1].1, "D4: 同物态内输出不应随 r 变化");
        assert_eq!(outs[1].1, outs[2].1, "D4: 同物态内输出不应随 r 变化");
    }

    // ---------------- D5 域护栏 ----------------
    #[test]
    fn d5_domain_guard() {
        assert_eq!(engine_for_state(state_of_flow_ratio(0.0)), Engine::Linear, "D5: r=0 ⇒ 固态");
        assert_eq!(engine_for_state(state_of_flow_ratio(9.0)), Engine::Expo, "D5: r=9 ⇒ 能量态");
        // 超出 clamp 上界也应稳定
        assert_eq!(engine_for_state(state_of_flow_ratio(100.0)), Engine::Expo);
    }

    // ---------------- D6 正反对照（判据不是恒真）----------------
    #[test]
    fn d6_negative_control() {
        // 若把期望写成 Expo，必须**不成立** —— 证明 D2 的期望值是有区分度的
        assert_ne!(
            engine_for_state(state_of_flow_ratio(1.05)),
            Engine::Expo,
            "D6: 1.05 是液态 ⇒ 不得为指数引擎"
        );
        assert_ne!(
            engine_for_state(state_of_flow_ratio(1.2)),
            Engine::Expo,
            "D6: 1.2 是气态 ⇒ 不得为指数引擎"
        );
        assert_ne!(
            engine_for_state(state_of_flow_ratio(0.8)),
            Engine::Linear,
            "D6: 0.8 是液态 ⇒ 不得为线性引擎"
        );
        // 反向对照：真正的切换点必须成立
        assert_eq!(engine_for_state(state_of_flow_ratio(1.2 + EPS)), Engine::Expo);
        assert_eq!(engine_for_state(state_of_flow_ratio(0.8 - EPS)), Engine::Linear);
    }

    // ---------------- D7 预算封顶（U1 裁定＝乙 的验收）----------------
    #[test]
    fn d7_budget_ceiling() {
        // 储备枯竭 ⇒ **无论比值多高**，物态被拉向固态 ⇒ 必为线性引擎
        assert_eq!(
            select_engine(&pool_high_ratio(0.0)),
            Engine::Linear,
            "D7: 储备为 0 ⇒ 必须封顶到固态"
        );
        // 储备充足 ⇒ 不封顶 ⇒ 跟随比值（高比值 ⇒ 能量态 ⇒ 指数）
        assert_eq!(
            select_engine(&pool_high_ratio(1.0)),
            Engine::Expo,
            "D7: 储备满 ⇒ 跟随比值"
        );
    }

    // ---------------- D7-b 等价性：储备充足时"预算态"退化为"比值态" ----------------
    #[test]
    fn d7b_budget_equals_ratio_when_stored_full() {
        let p = EnergyPool { flow_in: 0.9, flow_out: 1.0, stored: 1.0 };
        assert_eq!(
            select_engine(&p),
            engine_for_state(state_of_flow_ratio(p.ratio())),
            "D7-b: stored ≥ GOLDEN_RATIO 时，预算态应等于比值态"
        );
    }

    // ---------------- 结果面：三引擎在 x=0 的 0 锚点语义 ----------------
    #[test]
    fn d8_result_face_zero_anchor() {
        // 0 锚点：真空不自激 —— 三引擎在 x=0 的输出
        assert_eq!(step_with(Engine::Linear, 0.0), 0.01, "线性：0 ⊕ 0.01");
        assert_eq!(step_with(Engine::Fibonacci, 0.0), 0.0, "斐波那契：真空保持");
        assert_eq!(step_with(Engine::Expo, 0.0), 0.0, "指数：真空无法自激");
    }

    // ---------------- 结果面：组合接口一致 ----------------
    #[test]
    fn d9_select_and_step_consistency() {
        let p = pool_high_ratio(0.6); // stored=0.6 < 0.618 ⇒ 至多气态 ⇒ 斐波那契
        let (e, y) = select_and_step(&p, 0.3);
        assert_eq!(e, Engine::Fibonacci, "stored=0.6 ⇒ 至多气态 ⇒ 斐波那契");
        assert_eq!(y, step_with(Engine::Fibonacci, 0.3), "组合接口应与分步一致");
    }
}
