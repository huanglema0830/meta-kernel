//! **宿主机镜像测试**：把裸机断言（`kernel/src/verify.rs` 第 ⑥ 段）在 host 上原样跑一遍。
//!
//! **为什么需要**：裸机断言只能在 CI 的 QEMU 里失败，**失败一次要等一整轮 CI**才拿到一个编号。
//! 把它镜像到 host 后，**同一批断言本地就能跑**，且能区分两类原因：
//!   * host 也失败 ⇒ **逻辑错误**（期望值/实现理解错了）；
//!   * host 过、裸机红 ⇒ **ULP 敏感**（host 走 std 内在方法，no_std 走 `fmath`，差 1–2 ULP）。
//!
//! ⚠️ 本文件**不进保真度判据**（它是新增文件，不是对源文件的修改）。
//!
//! 注意：镜像测试**不能**断言"与裸机逐位相同"——两侧浮点实现不同（这正是要区分的那件事）。

use meta_kernel_core_nostd::{energy, interference, ontology, sanitizer, state};

/// 与裸机 ⑥ 段逐条对应的前置检查（镜像）。
#[test]
fn mirror_31_sanitizer() {
    assert_eq!(sanitizer::finalize(-1.0), 0.0);
    assert_eq!(sanitizer::finalize(0.5), 0.5, "合法值不得被改动");
    assert!(sanitizer::finalize(2.0) <= 1.0);
    assert_eq!(sanitizer::negative_to_zero(0.7), 0.7);
    assert_eq!(sanitizer::negative_to_zero(-0.7), 0.0);
}

/// 与裸机 33/34 对应：常量表 + 特征向量维度/值域 + 正反两侧必须不同。
#[test]
fn mirror_33_34_ontology() {
    assert_eq!(ontology::level_name(0), "黑");
    assert_eq!(ontology::level_name(3), "黄");
    assert_eq!(ontology::level_name(99), "白");

    let p = ontology::Pattern::new(vec![
        ontology::Element::new(1, 0.1),
        ontology::Element::new(4, 0.5),
        ontology::Element::new(4, 0.5),
        ontology::Element::new(7, 0.9),
    ]);
    let s = ontology::analyze(&p);
    println!("[诊断] p 的 analyze 长度 = {}（LEVELS = {}）", s.len(), ontology::LEVELS);
    for (i, v) in s.iter().enumerate() {
        println!("[诊断] s[{i}] = {v}");
    }
    assert_eq!(s.len(), ontology::LEVELS, "特征向量维度不符");
    for (i, v) in s.iter().enumerate() {
        assert!(
            !v.is_nan() && (0.0..=1.0).contains(v),
            "s[{i}] = {v} 越界或 NaN（裸机此处返回 34）"
        );
    }

    // ★ 反向断言：**结构不同**的输入必须给出**不同**的特征向量（防"算子空转"）。
    // ⚠️ **别拿"全黑 vs 全白"当反例**：实测二者输出**逐位相同** —— `analyze` 的 11 个分量是
    //    **特征轴（结构/关系）**，不是**层级振幅**；单元素模式挂哪一层都一样。
    //    （这正是 CI 上返回 **134 = 100+34** 的真根因：我**按函数名猜语义**写错了断言。）
    let p_black = ontology::Pattern::new(vec![ontology::Element::new(0, 1.0)]);
    let p_white = ontology::Pattern::new(vec![ontology::Element::new(10, 1.0)]);
    println!("[诊断] analyze(全黑) = {:?}", ontology::analyze(&p_black));
    println!("[诊断] analyze(全白) = {:?}", ontology::analyze(&p_white));

    let p_single = ontology::Pattern::new(vec![ontology::Element::new(4, 0.5)]);
    let a = ontology::analyze(&p);
    let b = ontology::analyze(&p_single);
    println!("[诊断] analyze(四元素) = {a:?}");
    println!("[诊断] analyze(单元素) = {b:?}");
    assert_ne!(a, b, "结构不同却输出相同 ⇒ 该断言在裸机上会返回 34");
}

/// 与裸机 35 对应。
#[test]
fn mirror_35_energy() {
    let p = ontology::Pattern::new(vec![
        ontology::Element::new(1, 0.1),
        ontology::Element::new(4, 0.5),
        ontology::Element::new(4, 0.5),
        ontology::Element::new(7, 0.9),
    ]);
    let e = energy::energy_level_evaluate(&p);
    println!("[诊断] energy_level_evaluate = {e}");
    assert!(!e.is_nan() && (0.0..=1.0).contains(&e), "e = {e} 越界（裸机返回 35）");
    assert!(matches!(energy::verdict_for(0.1), energy::Verdict::DecomposeToGranules));
    assert!(matches!(energy::verdict_for(0.9), energy::Verdict::Adopt));
}

/// 与裸机 36/37 对应。
#[test]
fn mirror_36_37_state_interference() {
    let hist: Vec<f64> = (0..8).map(|i| i as f64 / 8.0).collect();
    let ent = state::entropy_of_history(&hist);
    println!("[诊断] entropy_of_history = {ent}");
    assert!(!ent.is_nan() && (0.0..=1.0).contains(&ent), "ent = {ent} 越界（裸机返回 36）");
    assert_eq!(state::state_of_entropy(0.9).code(), 0);
    assert_eq!(state::state_of_entropy(0.0).code(), 3);
    assert_eq!(state::entropy_of_history(&[]), 1.0, "空历史应为 1.0");

    let wa: Vec<f32> = (0..32)
        .map(|i| (2.0 * core::f32::consts::PI * (i as f32) / 8.0).sin())
        .collect();
    let d = interference::phase_difference(&wa, &wa);
    println!("[诊断] phase_difference(自比) = {d}");
    assert!(d.is_finite() && d.abs() <= 1e-4, "d = {d}（裸机返回 37）");
}
