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

// ================== 片4（⑦ 段）镜像 ==================

use meta_kernel_core_nostd::{gene_library, habit, l1_field_parse, l5_evidence, trace};

/// 与裸机 41 对应：`trace::fingerprint_of` 的契约（空⇒0／确定／长度与能量流都要起作用）。
#[test]
fn mirror_41_trace_fingerprint() {
    let s4 = [0.2f32, 0.4, 0.6, 0.8];
    let s8 = [0.2f32, 0.4, 0.6, 0.8, 0.2, 0.4, 0.6, 0.8];
    assert_eq!(trace::fingerprint_of(&[], 0.5), 0, "空输入必须为 0");
    assert_eq!(trace::fingerprint_of(&s4, 0.5), trace::fingerprint_of(&s4, 0.5), "必须确定性");
    assert_ne!(trace::fingerprint_of(&s4, 0.5), trace::fingerprint_of(&s8, 0.5), "长度须入指纹");
    assert_ne!(trace::fingerprint_of(&s4, 0.0), trace::fingerprint_of(&s4, 1.0), "能量流须入指纹");
}

/// 与裸机 42 对应：`normalize` 的半饱和点契约 + 负值归零 + 单调。
#[test]
fn mirror_42_normalize() {
    let k = l1_field_parse::K_TEXT_CHARS;
    assert_eq!(l1_field_parse::normalize(0.0, k), 0.0);
    assert_eq!(l1_field_parse::normalize(k, k), 0.5, "达到 k 时必须恰为 0.5");
    assert_eq!(l1_field_parse::normalize(-5.0, k), 0.0);
    assert!(l1_field_parse::normalize(100.0, 2000.0) < l1_field_parse::normalize(500.0, 2000.0));
}

/// 与裸机 43 对应：`habit_strength` 的边界与单调性。
#[test]
fn mirror_43_habit_strength() {
    assert_eq!(habit::habit_strength(0, 1.0), 0.0);
    let (h1, h10, h100) = (
        habit::habit_strength(1, 1.0),
        habit::habit_strength(10, 1.0),
        habit::habit_strength(100, 1.0),
    );
    println!("[诊断] habit_strength 1/10/100 = {h1}/{h10}/{h100}");
    assert!((0.0..=1.0).contains(&h1) && (0.0..=1.0).contains(&h100));
    assert!(h1 < h10 && h10 < h100);
}

/// 与裸机 44 对应：`fnv1a64` 的确定性与区分度。
#[test]
fn mirror_44_fnv1a64() {
    let k1 = gene_library::fnv1a64(0, b"meta-kernel");
    assert_eq!(k1, gene_library::fnv1a64(0, b"meta-kernel"));
    assert_ne!(k1, gene_library::fnv1a64(0, b"meta-kernal"));
    assert_ne!(k1, gene_library::fnv1a64(1, b"meta-kernel"));
}

/// 与裸机 45 对应：`l5_evidence::adjust` 的**中性点 m = 0.5**（**R7 教训**）。
#[test]
fn mirror_45_evidence_neutral_point() {
    let g = l5_evidence::Gains::default();
    let neutral = l5_evidence::Evidence { world_match: Some(0.5), prediction_error: None };
    let a = l5_evidence::adjust(0.7, &neutral, &g);
    println!("[诊断] m=0.5 ⇒ world_adjust={} final={}", a.world_adjust, a.final_confidence);
    assert_eq!(a.world_adjust, 0.0, "m=0.5 时修正量必须恰为 0");
    assert_eq!(a.world_match, Some(0.5));
    let up = l5_evidence::adjust(
        0.7,
        &l5_evidence::Evidence { world_match: Some(1.0), prediction_error: None },
        &g,
    );
    let dn = l5_evidence::adjust(
        0.7,
        &l5_evidence::Evidence { world_match: Some(0.0), prediction_error: None },
        &g,
    );
    println!("[诊断] m=1.0 ⇒ {}；m=0.0 ⇒ {}", up.world_adjust, dn.world_adjust);
    assert!(up.world_adjust > 0.0 && dn.world_adjust < 0.0, "两端必须异号且非零");
    assert!((0.0..=1.0).contains(&a.final_confidence) && (0.0..=1.0).contains(&up.final_confidence));
}

// ================= 片5（⑧ 段）的镜像：先在 host 跑出真实值，再编码成裸机断言 =================

#[test]
fn mirror_shard5_compare_three_bands() {
    use meta_kernel_core_nostd::{l5_baseline::BaselineField, l5_compare};
    let base = BaselineField {
        earth: 0.7, water: 0.7, fire: 0.7, wind: 0.7, object: "s8", established: "e",
    };
    let ping = l5_compare::compare(&[0.7; 4], &base);
    let kang = l5_compare::compare(&[1.4; 4], &base);   // dev = |1.4/0.7| = 2.0 > 1.618 ⇒ 亢
    let ku = l5_compare::compare(&[0.35; 4], &base);   // dev = |0.35/0.7| = 0.5 < 0.618 ⇒ 枯
    println!("[诊断] 平={:?}", ping.map(|b| b.code()));
    println!("[诊断] 亢(2.0x)={:?}", kang.map(|b| b.code()));
    println!("[诊断] 枯(0.5x)={:?}", ku.map(|b| b.code()));
    assert!(ping.iter().all(|b| *b == l5_compare::Band::Ping), "本底自比必须全平（契约）");
    assert!(kang.iter().all(|b| *b == l5_compare::Band::Kang), "2 倍必须全亢");
    assert!(ku.iter().all(|b| *b == l5_compare::Band::Ku), "半倍必须全枯");
}

#[test]
fn mirror_shard5_ledger_chain() {
    use meta_kernel_core_nostd::l7::grade::Grade;
    use meta_kernel_core_nostd::l7::ledger::{ActionLedger, GENESIS};
    let mut l = ActionLedger::new();
    println!("[诊断] 空账 len={} head={} verify={}", l.len(), l.head(), l.verify());
    assert_eq!(l.len(), 0);
    assert_eq!(l.head(), GENESIS, "空账的链锚必须是 GENESIS");
    assert!(l.verify(), "空账必须自洽");
    l.append(7, Grade::T0Read, "suggested", "n1");
    l.append(8, Grade::T2Confirm, "confirmed", "n2");
    println!("[诊断] 两条 len={} head={:#x} verify={}", l.len(), l.head(), l.verify());
    assert_eq!(l.len(), 2);
    assert_ne!(l.head(), GENESIS, "有记录后链锚必须变");
    assert!(l.verify());
    // 反向断言：篡改一条 ⇒ verify 必须为 false（否则 verify 是空转）
    let h = l.links[0].hash;
    l.links[0].hash = h ^ 1;
    println!("[诊断] 篡改后 verify={}", l.verify());
    assert!(!l.verify(), "篡改后仍通过 ⇒ verify 空转，判据失效");
    l.links[0].hash = h;
    assert!(l.verify(), "复原后必须再次自洽");
}

#[test]
fn mirror_shard5_hourglass() {
    use meta_kernel_core_nostd::hourglass::BubbleHourglass;
    let mut hg = BubbleHourglass::with_caps(2, 2, 2, 1);
    for v in [0.1f32, 0.2, 0.3] {
        hg.push(v);
    }
    let out = hg.tick(None);
    println!("[诊断] tick 输出粒数={} backlog={}", out.len(), hg.backlog());
    assert!(out.len() <= 1, "契约：每 tick 至多放行 1 粒");
    for v in &out {
        assert!((0.0..=1.0).contains(v), "放行的种子必须在 0-1");
    }
}

// ============ ⑨ 段（2.3b 片6）· 与 `kernel/src/verify.rs::self_check_shard6` **逐条对应** ============
//
// ⚠️ 本段的意义：片6 含**本仓唯一的「类型替换」**（`HashMap` → `BTreeMap`）。
// 下面 92/93 两条**直接证成「换容器后 `get`/`insert` 语义不变」**——
// 这不是"编译过了就算"，而是**用可证伪的契约**证明**行为等价**。

/// 91：孪生指纹可逆，且永不恒等。
#[test]
fn mirror_shard6_91_twin_fingerprint() {
    use meta_kernel_core_nostd::positive_source::twin_fingerprint;
    for x in [0u64, 1, 0xDEAD_BEEF_u64, u64::MAX, 0x8000_0000_0000_0000] {
        let t = twin_fingerprint(x);
        println!("[诊断] x={x:#018x} twin={t:#018x} twin(twin)={:#018x}", twin_fingerprint(t));
        assert_eq!(twin_fingerprint(t), x, "twin 必须可逆（twin(twin(x))==x）");
        assert_ne!(t, x, "twin(x) 不得恒等（否则配对无意义）");
    }
}

/// 92/93：孪生索引往返 + 幂等 —— **本片「类型替换」的行为等价证明**。
#[test]
fn mirror_shard6_92_93_twin_index_roundtrip() {
    use meta_kernel_core_nostd::positive_source::{twin_fingerprint, PositiveSource};
    let mut ps = PositiveSource::new();
    let fp: u64 = 0x0123_4567_89AB_CDEF;

    ps.entangle(fp, 0.4);
    println!("[诊断] entangle 后 len={}", ps.entangled_len());
    assert_eq!(ps.entangled_len(), 1, "首次登记必须入库");

    // 用「孪生键」查得回（走 `twin_index.get`）
    let got = ps.entanglement_match(twin_fingerprint(fp));
    println!("[诊断] 用孪生键查 => {:?}", got);
    assert_eq!(got, Some(0.4), "插入后必须能用孪生键查回");

    // 幂等：同正指纹再登记 ⇒ 条目数不变（`get` 命中分支），但补充增量更新
    ps.entangle(fp, 0.9);
    println!("[诊断] 二次 entangle 后 len={} 值={:?}", ps.entangled_len(), ps.entanglement_match(twin_fingerprint(fp)));
    assert_eq!(ps.entangled_len(), 1, "幂等破 ⇒ `get` 命中逻辑失效（换容器最可能伤到这里）");
    assert_eq!(ps.entanglement_match(twin_fingerprint(fp)), Some(0.9), "更新必须生效");

    // 反向断言：**未登记**的孪生键必须查不到（否则配对退化成"永远命中"）
    assert_eq!(ps.entanglement_match(fp), None, "未登记的孪生键竟能命中 ⇒ 配对失效");
}

/// 94：诊断中性点（输入 = 本底 ⇒ 全场平 ⇒ `advice.calm`；R7 教训）。
#[test]
fn mirror_shard6_94_diagnosis_neutral_point() {
    use meta_kernel_core_nostd::l5_baseline::BaselineField;
    use meta_kernel_core_nostd::l5_compare::Band;
    use meta_kernel_core_nostd::l5_diagnosis;
    let base = BaselineField {
        earth: 0.6,
        water: 0.6,
        fire: 0.6,
        wind: 0.6,
        object: "self-check",
        established: "self-check",
    };
    let c = l5_diagnosis::synthesize(&[0.6; 4], &base, &[Band::Ping; 4]);
    println!("[诊断] key={} text={} conf={}", c.suggestion_key, c.suggestion, c.confidence);
    assert_eq!(c.suggestion_key, "advice.calm", "中性点必须映射到「维持现状」⇒ 否则任何输入都被报成异常");
    assert!(!c.suggestion.is_empty(), "默认文本不得为空（`String`/`format!` 路径须真正工作）");
}

/// 95：动作白名单（在册 id 可取；**不在册必 None**）。
#[test]
fn mirror_shard6_95_action_catalog_whitelist() {
    use meta_kernel_core_nostd::l7::repair;
    for id in 1u32..=4 {
        let a = repair::action_by_id(id).unwrap_or_else(|| panic!("id={id} 在册却取不到"));
        assert_eq!(a.id, id);
    }
    for id in [0u32, 5, 99, u32::MAX] {
        assert!(repair::action_by_id(id).is_none(), "id={id} 不在白名单却取到了 ⇒「不侵」的编译期目录失效");
    }
    assert!(repair::action_by_key("clean-temp").is_some(), "稳定键查不到 ⇒ 宿主无法对表执行");
}

/// 附：L1 视觉映射最小存在性（片6 第四模块）。
#[test]
fn mirror_shard6_l1_mapping_finite() {
    use meta_kernel_core_nostd::l1_mapping;
    let g = l1_mapping::GaborParams::default();
    println!("[诊断] Gabor λ={} θ={} σ={} γ={}", g.lambda, g.theta, g.sigma, g.gamma);
    assert!(g.lambda.is_finite() && g.theta.is_finite() && g.sigma.is_finite() && g.gamma.is_finite());
    assert!(l1_mapping::GABOR_DEFAULTS.iter().all(|v| v.is_finite()));
}
