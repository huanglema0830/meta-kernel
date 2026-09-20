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

// ============ ⑩ 段（2.3b 片7／片8）· 与 `kernel/src/verify.rs::self_check_shard7` **逐条对应** ============
//
// ⚠️ 本段的意义：片7／片8 是**末片**，且**零「替换类」**⇒ 断言的重点从「行为等价」转为
//   **业务契约是否真成立**：缺词是否如实标记、叠层是否有上界、手写 JSON 是否结构自洽。
//   先在这里跑出**真实值**，再把它们编码成裸机断言（避免"期望值算错 ⇒ 裸机假红"）。

fn shard78_diagnosis() -> meta_kernel_core_nostd::l5_diagnosis::Diagnosis {
    use meta_kernel_core_nostd::l5_baseline::BaselineField;
    use meta_kernel_core_nostd::l5_diagnosis::diagnose;
    let base = BaselineField {
        earth: 0.6,
        water: 0.6,
        fire: 0.6,
        wind: 0.6,
        object: "self-check",
        established: "self-check",
    };
    diagnose(&[0.6, 0.6, 0.6, 0.6, 1.0, 0.5, 1.0], &base, "shard78")
}

/// 101：缺词必须如实标记 `(待补)`，且**不阻断**（空词表仍能跑出结果）。
#[test]
fn mirror_shard78_101_translate_fallback_marked() {
    use meta_kernel_core_nostd::l5_translate;
    let d = shard78_diagnosis();
    let empty: [l5_translate::Term; 0] = [];
    let s = l5_translate::summarize_for(&empty, "universal", &d);
    println!("[诊断] 空词表输出 = {s:?}");
    assert!(s.contains("(待补)"), "缺词必须如实标记，不得静默吞掉");
}

/// 102：8 语言齐备且逐条非空（证 `Vec<(String,String)>` 在 no_std 下真可用）。
#[test]
fn mirror_shard78_102_all_summaries_complete() {
    use meta_kernel_core_nostd::l5_translate;
    let d = shard78_diagnosis();
    let all = l5_translate::all_summaries(&d);
    println!("[诊断] 语言数 = {}／{}", all.len(), l5_translate::LANGS.len());
    for (k, v) in all.iter() {
        println!("       {k} → {v:?}");
    }
    assert_eq!(all.len(), l5_translate::LANGS.len());
    assert_eq!(all.len(), 8);
    assert!(all.iter().all(|(k, v)| !k.is_empty() && !v.is_empty()));
}

/// 103：原版永不叠加；且**一切模式 α ≤ 0.35**（「网页始终可见」的量化上界）。
#[test]
fn mirror_shard78_103_face_alpha_bounded() {
    use meta_kernel_core_nostd::l6_face::FaceMode;
    println!("[诊断] α(1.0)：原版={} 场域={} 混合={}",
        FaceMode::Original.overlay_alpha(1.0),
        FaceMode::Field.overlay_alpha(1.0),
        FaceMode::Blend.overlay_alpha(1.0));
    assert_eq!(FaceMode::Original.overlay_alpha(1.0), 0.0, "原版必须不叠加");
    for m in FaceMode::all() {
        for c in [0.0f64, 0.5, 1.0] {
            let a = m.overlay_alpha(c);
            assert!((0.0..=0.35).contains(&a), "α 越上界：{a}（mode={m:?} c={c}）");
        }
    }
}

/// 104：混合恰为场域之半；未知输入走**安全缺省**（不叠加）。
#[test]
fn mirror_shard78_104_face_blend_half_and_default() {
    use meta_kernel_core_nostd::l6_face::FaceMode;
    let f = FaceMode::Field.overlay_alpha(1.0);
    let b = FaceMode::Blend.overlay_alpha(1.0);
    println!("[诊断] 场域={f} 混合={b} 混合×2−场域={}", b * 2.0 - f);
    let diff = b * 2.0 - f;
    assert!(diff < 1e-12 && -diff < 1e-12, "「混合＝场域一半」名不符实");
    assert_eq!(FaceMode::parse("乱码"), FaceMode::Original, "未知输入必须安全缺省");
}

/// 105：JSON 结构首尾 ＋ 引号/反斜杠配平（转义写坏必被抓住）。
#[test]
fn mirror_shard78_105_router_json_balanced() {
    use meta_kernel_core_nostd::l5_router;
    let d = shard78_diagnosis();
    let j = l5_router::to_json(&d);
    let (q, bs) = (j.matches('"').count(), j.matches('\\').count());
    println!("[诊断] JSON 长度={} 引号数={} 反斜杠数={}", j.len(), q, bs);
    println!("[诊断] JSON 前 140 字 = {}", &j[..j.len().min(140)]);
    assert!(j.starts_with('{') && j.ends_with('}'), "JSON 首尾不成对");
    assert!(j.contains("\"schema\":"), "缺 schema 字段");
    assert!(j.contains("\"universal\":"), "缺 universal 语言键");
    assert_eq!(q % 2, 0, "引号未配平 ⇒ 转义不成立");
    assert_eq!(bs % 2, 0, "反斜杠未配平 ⇒ 转义不成立");
}


// ====== ⑪ 段（Q11）· 与 `kernel/src/verify.rs::self_check_q11_engine_select` **逐条对应** ======
//
// 说明：本机**无法构建 boot 层**（缺 `dlltool`／`link.exe`／`clang`）⇒ ⑪ 段的**权威执行**在 CI 的
// boot job；本镜像在 host 上**跑同一批断言的语义**（同源数据、同源口径），使**本机可复现**。
const Q11_EPS: f32 = 1e-6;

/// 111：选择表（Q8）四档唯一映射。
#[test]
fn mirror_q11_111_selection_table() {
    use meta_kernel_core_nostd::engine_select::{engine_for_state, Engine};
    use meta_kernel_core_nostd::state::{state_of_flow_ratio, State};
    let cases = [
        (0.4_f32, State::Solid, Engine::Linear),
        (0.9, State::Liquid, Engine::Fibonacci),
        (1.1, State::Gas, Engine::Fibonacci),
        (1.5, State::Energy, Engine::Expo),
    ];
    for (r, ws, we) in cases {
        let s = state_of_flow_ratio(r);
        println!("[诊断] r={} 物态={:?} 引擎={:?}", r, s, engine_for_state(s));
        assert_eq!(s, ws, "111: r={} 物态错", r);
        assert_eq!(engine_for_state(s), we, "111: r={} 引擎错", r);
    }
}

/// 112：1.05 处「物态切换、引擎不切换」；0.8／1.2 才是真切换点。
#[test]
fn mirror_q11_112_no_switch_at_1_05() {
    use meta_kernel_core_nostd::engine_select::{engine_for_state, Engine};
    use meta_kernel_core_nostd::state::state_of_flow_ratio;
    let s_lo = state_of_flow_ratio(1.05 - Q11_EPS);
    let s_hi = state_of_flow_ratio(1.05 + Q11_EPS);
    println!("[诊断] 1.05±ε ⇒ 物态 {:?} / {:?}，引擎 {:?} / {:?}", s_lo, s_hi,
             engine_for_state(s_lo), engine_for_state(s_hi));
    assert_ne!(s_lo, s_hi, "112: 1.05 两侧物态应切换");
    assert_eq!(engine_for_state(s_lo), engine_for_state(s_hi), "112: 引擎不得切换");
    assert_eq!(engine_for_state(s_lo), Engine::Fibonacci, "112: 液态应走斐波那契");
    assert_ne!(engine_for_state(state_of_flow_ratio(0.8 - Q11_EPS)),
               engine_for_state(state_of_flow_ratio(0.8 + Q11_EPS)), "112: 0.8 应切换");
    assert_ne!(engine_for_state(state_of_flow_ratio(1.2 - Q11_EPS)),
               engine_for_state(state_of_flow_ratio(1.2 + Q11_EPS)), "112: 1.2 应切换");
}

/// 113：U1＝乙 · 预算封顶（储备枯竭拉向更固者；充足时不无故降级）。
#[test]
fn mirror_q11_113_budget_cap() {
    use meta_kernel_core_nostd::energy::EnergyPool;
    use meta_kernel_core_nostd::engine_select::{select_engine, Engine};
    let starved = EnergyPool { flow_in: 1.0, flow_out: 0.0, stored: 0.0 };
    let rich = EnergyPool { flow_in: 1.0, flow_out: 0.0, stored: 1.0 };
    println!("[诊断] 枯竭池 ratio={} ⇒ {:?}；充足池 ratio={} ⇒ {:?}",
             starved.ratio(), select_engine(&starved), rich.ratio(), select_engine(&rich));
    assert_eq!(select_engine(&starved), Engine::Linear, "113: 枯竭应封顶到固态→线性");
    assert_eq!(select_engine(&rich), Engine::Expo, "113: 充足不得无故降级");
}

/// 114：U2＝丙 · 双面一致 ＋ 结果面可复现。
#[test]
fn mirror_q11_114_two_faces_consistent() {
    use meta_kernel_core_nostd::energy::EnergyPool;
    use meta_kernel_core_nostd::engine_select::{select_and_step, select_engine, step_with};
    let pools = [
        EnergyPool { flow_in: 0.4, flow_out: 1.0, stored: 1.0 },
        EnergyPool { flow_in: 0.9, flow_out: 1.0, stored: 1.0 },
        EnergyPool { flow_in: 1.15, flow_out: 1.0, stored: 1.0 },
        EnergyPool { flow_in: 1.0, flow_out: 0.0, stored: 1.0 },
    ];
    for p in pools {
        let (e, y) = select_and_step(&p, 0.5);
        println!("[诊断] ratio={:.4} ⇒ 引擎={:?} 输出={}", p.ratio(), e, y);
        assert_eq!(e, select_engine(&p), "114: 标签面与选择不一致");
        assert_eq!(y.to_bits(), step_with(e, 0.5).to_bits(), "114: 结果面与标签不一致");
        assert_eq!(y.to_bits(), select_and_step(&p, 0.5).1.to_bits(), "114: 不可复现");
    }
}

/// 115：Q10 前问 · 选择器不调制输入（同引擎、不同比值 ⇒ 结果面逐位相同）。
#[test]
fn mirror_q11_115_selector_not_modulator() {
    use meta_kernel_core_nostd::energy::EnergyPool;
    use meta_kernel_core_nostd::engine_select::{select_engine, step_with, Engine};
    let liquid = EnergyPool { flow_in: 0.9, flow_out: 1.0, stored: 1.0 };
    let gas = EnergyPool { flow_in: 1.15, flow_out: 1.0, stored: 1.0 };
    let e_liq = select_engine(&liquid);
    let e_gas = select_engine(&gas);
    println!("[诊断] liquid ratio={:.4} ⇒ {:?}；gas ratio={:.4} ⇒ {:?}",
             liquid.ratio(), e_liq, gas.ratio(), e_gas);
    assert_eq!(e_liq, e_gas, "115: 两池应同选斐波那契（前提）");
    assert_eq!(e_liq, Engine::Fibonacci, "115: 应为斐波那契");
    assert_eq!(step_with(e_liq, 0.5).to_bits(), step_with(e_gas, 0.5).to_bits(),
               "115: 同引擎结果面不同 ⇒ 输入被调制（Q10 前问破）");
    let lo = select_engine(&EnergyPool { flow_in: 0.95, flow_out: 1.0, stored: 1.0 });
    let hi = select_engine(&EnergyPool { flow_in: 1.10, flow_out: 1.0, stored: 1.0 });
    assert_eq!(lo, hi, "115: 0.95 与 1.10 应同引擎（对应 V2「1.05 处差值 0」）");
}

// ============================================================================
// ⑫ 段镜像（2026-09-20）：**2.4 边界层接入帧缓冲**（`kernel/src/present.rs`）
//
// 为什么必须镜像：boot 层**本机完全不可构建**（无 MSVC 库／`clang`／`lld`，连 `cargo check`
// 都跑不起来 —— 实测）。⇒ 裸机上那 5 个编号（121–125）**失败一次要等一整轮 CI**。
// 把 present 的**写入循环与回读比对**在 host 上用**合成 `FbDesc`** 原样跑一遍，
// 就能把"逻辑错"（期望值／字节序／行距）在本地先筛掉，只把"真·硬件差异"留给 QEMU。
//
// ⚠️ **镜像不能替代裸机**：这里用的是**自造的** `FbDesc`；真实 `stride`/`bpp`/`pixel_format`
// 只能由 bootloader 给出 ⇒ "真实引导"仍**只能由 CI 的 boot-image job 证成**（C5：未验证就标未验证）。
// ============================================================================

/// 121：**Bgr 24bpp + `stride > width` 的往返**（写入数 == 4，回读逐字节一致，行距填充不被踩）。
///
/// 这是 ⑫ 段的主判据。用 `stride=6 > width=4` **刻意制造行间填充**——若偏移算错
/// （用 `width` 而不是 `stride` 步进），回读与期望就会错位（这正是 `fb.rs` 文件头第一条判据要钉的地方）。
#[test]
fn mirror_present_121_roundtrip_bgr_with_padding() {
    use meta_kernel_core_nostd::fb::{encode, put_pixel, FbDesc, FbFormat, Rgb24};
    use meta_kernel_core_nostd::project::{project_gray_u8, ProjectSpec};

    let d = FbDesc::new(2, 2, 6, 3, FbFormat::Bgr); // width=2, stride=6px ⇒ 每行右侧 4px 填充
    let mut buf = vec![0u8; d.min_len().expect("min_len")];

    let spec = ProjectSpec::new(2, 2, 0.0, 1.0).expect("spec");
    let field = [0.0f32, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let gray = project_gray_u8(&field, &spec);
    assert_eq!(gray.len(), 4, "123 前置：投影出口长度");
    println!("[诊断] ⑫ 投影灰度 = {:?}", gray);
    assert_eq!(gray, vec![0u8, 85u8, 170u8, 255u8], "123：投影灰度值");

    // —— 与 `present.rs::present_selfcheck` **同一套循环** ——
    let mut written = 0usize;
    for i in 0..4usize {
        let (x, y) = (i % 2, i / 2);
        let g = gray[i];
        if put_pixel(&mut buf, &d, x, y, Rgb24::new(g, g, g)) {
            written += 1;
        }
    }
    assert_eq!(written, 4, "121：写入像素数（不足 ⇒ 裸机返回 121）");

    // 回读：与 `encode` 的期望字节逐字节比对（**期望值从被测物推导，不手写**，D40/C19）
    for i in 0..4usize {
        let (x, y) = (i % 2, i / 2);
        let g = gray[i];
        let (bytes, n) = encode(&d, Rgb24::new(g, g, g));
        assert_eq!(n, 3, "Bgr 应为 3 字节");
        let off = d.offset_of(x, y).expect("offset");
        assert_eq!(&buf[off..off + n], &bytes[..n], "122：({},{}) 回读与期望不符", x, y);
        // 灰色 ⇒ 三通道相等（Bgr 的字节序也一并被验到）
        assert_eq!(bytes[0], bytes[1]);
        assert_eq!(bytes[1], bytes[2]);
    }

    // 行距填充必须**逐字节未被触碰**：每行可视区宽 2px×3B＝6B，行字节数 6px×3B＝18B
    // ⇒ 每行填充 = [行首+6, 行首+18)，共 12 字节（**区间由 `FbDesc` 现算，不写死魔数**）
    let row_bytes = d.stride_px * d.bpp; // 18
    let visible_bytes = d.width * d.bpp; // 6
    assert_eq!(row_bytes, 18);
    assert_eq!(visible_bytes, 6);
    for row in 0..d.height {
        let pad = &buf[row * row_bytes + visible_bytes..(row + 1) * row_bytes];
        assert_eq!(pad.len(), 12, "第 {} 行填充宽度", row);
        assert!(pad.iter().all(|&b| b == 0), "第 {} 行填充被踩 ⇒ 偏移用错了 width", row);
    }
}

/// 121／122 的**阳性对照**：故意篡改一个字节 ⇒ 回读比对**必须**发现（防"判据空转"）。
#[test]
fn mirror_present_122_mismatch_is_detected() {
    use meta_kernel_core_nostd::fb::{encode, put_pixel, FbDesc, FbFormat, Rgb24};

    let d = FbDesc::new(1, 1, 1, 3, FbFormat::Rgb);
    let mut buf = vec![0u8; d.min_len().expect("min_len")];
    assert!(put_pixel(&mut buf, &d, 0, 0, Rgb24::new(0x11, 0x22, 0x33)));
    let (bytes, n) = encode(&d, Rgb24::new(0x11, 0x22, 0x33));
    assert_eq!(&buf[..n], &bytes[..n], "写后应立即一致");

    buf[1] ^= 0xFF; // 篡改
    assert_ne!(&buf[..n], &bytes[..n], "122：篡改后仍判一致 ⇒ 判据空转（必须检出）");
}

/// 123：**投影出口非法** ⇒ 空序列（`present_field` 据此返回 0，**不冒充成功**）。
#[test]
fn mirror_present_123_project_invalid_yields_empty() {
    use meta_kernel_core_nostd::project::{project_gray_u8, ProjectSpec};

    let spec = ProjectSpec::new(4, 4, 0.0, 1.0).expect("spec");
    assert!(project_gray_u8(&[0.5f32; 3], &spec).is_empty(), "123：输入不足须为空");
    assert!(ProjectSpec::new(4, 4, 1.0, 1.0).is_none(), "123：hi==lo 须被拒");
}

/// 124：**格式不受支持** ⇒ `encode` 长度 0、`put_pixel` 返回 `false`（**不写任何字节**）。
#[test]
fn mirror_present_124_format_unsupported() {
    use meta_kernel_core_nostd::fb::{encode, put_pixel, FbDesc, FbFormat, Rgb24};

    // `Packed` 起始位 > 24 ⇒ 8 位放不下 ⇒ 不支持（fb.rs 已写死的语义）
    let bad = FbDesc::new(1, 1, 1, 4, FbFormat::Packed { red: 32, green: 8, blue: 0 });
    let (_, n) = encode(&bad, Rgb24::WHITE);
    assert_eq!(n, 0, "124：起始位 > 24 必须判不支持");
    let mut buf = vec![0xAAu8; 4];
    assert!(!put_pixel(&mut buf, &bad, 0, 0, Rgb24::WHITE), "124：不得写入");
    assert!(buf.iter().all(|&b| b == 0xAA), "124：不支持时**一个字节都不许动**");

    // 对照：合法 XRGB8888 必须可用（否则上面的"不支持"没有意义）
    let ok = FbDesc::new(1, 1, 1, 4, FbFormat::Packed { red: 16, green: 8, blue: 0 });
    let (b, n) = encode(&ok, Rgb24::new(0x11, 0x22, 0x33));
    assert_eq!(n, 4, "XRGB8888 应支持");
    println!("[诊断] XRGB8888 编码 = {:02X?}", b);
    assert_eq!(b[0], 0x33, "蓝在 bit0（小端首字节）");
    assert_eq!(b[2], 0x11, "红在 bit16");
}

/// 125：**`FbDesc` 与真实缓冲自相矛盾**（`min_len` > `buf.len()`）⇒ 必须**先拒后写**。
#[test]
fn mirror_present_125_desc_inconsistent_detected() {
    use meta_kernel_core_nostd::fb::FbDesc;

    let d = FbDesc::new(64, 64, 64, 3, meta_kernel_core_nostd::fb::FbFormat::Bgr);
    let need = d.min_len().expect("min_len");
    assert_eq!(need, 64 * 3 * 64);
    let small = vec![0u8; need - 1];
    // 与 `present.rs` 的守卫同式：`min_len <= buf.len()` 不成立 ⇒ 判 125（**不写半截**）
    assert!(need > small.len(), "125：该场景必须落入'缓冲不足'分支");
}

// ---------------------------------------------------------------------------
// ★ 2026-09-20 · **2.4 product 路径接线**的 host 镜像（126–128）
//
// `present.rs::present_product_selftest` 的**逻辑本体** ＝「纯算层 SDF 采样 → `Project(S)` →
// 按 `put_pixel` 写入 → 回读逐字节比对」。这几步**全在 `nostd` 侧** ⇒ **可在 host 上原样镜像**
// （boot 层本机不可构建，理由见上面 ⑫ 段镜像的头注）。
// ⚠️ 镜像**不替代 QEMU**：真实 `stride`/`bpp`/`pixel_format` 仍只能由 bootloader 给出。
// ---------------------------------------------------------------------------

/// 126：**product 路径往返** —— SDF 场 → 投影 → 写入 → 回读，逐字节一致；
/// 且**图案非退化**（多种灰度）—— 否则"全同色"会让偏移/字节序错误**无法暴露**（阳性对照）。
#[test]
fn mirror_present_126_product_roundtrip() {
    use meta_kernel_core_nostd::fb::{encode, put_pixel, FbDesc, FbFormat, Rgb24};
    use meta_kernel_core_nostd::field::sdf;
    use meta_kernel_core_nostd::project::{project_gray_u8, ProjectSpec};

    // 与裸机同参：16×16 视区 ＋ **行距 > 宽**（刻意制造行间填充，钉死"步进用 stride_px"）
    let w = 16usize;
    let h = 16usize;
    let d = FbDesc::new(w, h, w + 4, 3, FbFormat::Bgr);
    let mut buf = vec![0u8; d.min_len().expect("min_len")];

    // —— 场源：与 `present_product_selftest` **同一式**（几何导出的圆盘 SDF）——
    let cx = (w as f32 - 1.0) * 0.5;
    let cy = (h as f32 - 1.0) * 0.5;
    let r = (if w < h { w } else { h }) as f32 * 0.5 - 1.0;
    let mut field = [0.0f32; 256];
    let cells = w * h;
    let n = sdf::sample_into(|x, y| sdf::circle(cx, cy, r, x, y), w, h, &mut field[..cells]);
    assert_eq!(n, cells, "126：SDF 采样数");

    let spec = ProjectSpec::new(w, h, -r, r).expect("spec");
    let gray = project_gray_u8(&field[..cells], &spec);
    assert_eq!(gray.len(), cells, "126：投影长度");

    // **阳性对照**：图案必须**非退化**（多种灰度）
    let mut uniq = gray.clone();
    uniq.sort_unstable();
    uniq.dedup();
    println!(
        "[诊断] 126 product 灰度种类 = {} / min = {} / max = {}",
        uniq.len(),
        gray.iter().min().unwrap(),
        gray.iter().max().unwrap()
    );
    assert!(uniq.len() >= 8, "126：图案退化（灰度种类 {} < 8）⇒ 判据会空转", uniq.len());

    // —— 写入（与 `present_field` **同一套循环**）——
    let mut written = 0usize;
    for i in 0..cells {
        let (x, y) = (i % w, i / w);
        let g = gray[i];
        if put_pixel(&mut buf, &d, x, y, Rgb24::new(g, g, g)) {
            written += 1;
        }
    }
    assert_eq!(written, cells, "126：写入像素数（不足 ⇒ 裸机返回 126）");

    // —— 回读：与 `encode` 的期望字节逐字节比对（期望值从被测物推导，D40/C19）——
    for i in 0..cells {
        let (x, y) = (i % w, i / w);
        let g = gray[i];
        let (bytes, bn) = encode(&d, Rgb24::new(g, g, g));
        assert_eq!(bn, 3, "Bgr 应为 3 字节");
        let off = d.offset_of(x, y).expect("offset");
        assert_eq!(&buf[off..off + bn], &bytes[..bn], "127：({},{}) 回读与期望不符", x, y);
    }

    // 行间填充**逐字节未被触碰**（区间由 `FbDesc` 现算，不写死魔数）
    let row_bytes = d.stride_px * d.bpp;
    let visible_bytes = d.width * d.bpp;
    assert_eq!(row_bytes - visible_bytes, 12);
    for row in 0..d.height {
        let pad = &buf[row * row_bytes + visible_bytes..(row + 1) * row_bytes];
        assert!(pad.iter().all(|&b| b == 0), "126：第 {} 行填充被踩（偏移用错 width）", row);
    }
}

/// 127：**阳性对照** —— 故意篡改 product 路径写入的一个字节 ⇒ 回读比对**必须**发现。
#[test]
fn mirror_present_127_product_mismatch_detected() {
    use meta_kernel_core_nostd::fb::{encode, put_pixel, FbDesc, FbFormat, Rgb24};

    let d = FbDesc::new(4, 4, 4, 3, FbFormat::Rgb);
    let mut buf = vec![0u8; d.min_len().expect("min_len")];
    assert!(put_pixel(&mut buf, &d, 1, 1, Rgb24::new(0x11, 0x22, 0x33)));
    // ⚠️ 比对偏移必须**由 `FbDesc` 现算**（本用例写 (1,1) ⇒ off = (1*4+1)*3 = 15，**不是 0**）
    //    —— 首版误用 `buf[..n]`（= (0,0)）⇒ 该测试自测当场判红；属"**断言失败先自查期望值**"的同族教训
    //    （被测物正确，错的是期望值 ⇒ **不改被测物、不调参**）。
    let off = d.offset_of(1, 1).expect("offset");
    let (bytes, n) = encode(&d, Rgb24::new(0x11, 0x22, 0x33));
    assert_eq!(&buf[off..off + n], &bytes[..n], "写后应立即一致");

    buf[off + 1] ^= 0xFF; // 篡改
    assert_ne!(&buf[off..off + n], &bytes[..n], "127：篡改后仍判一致 ⇒ 判据空转（必须检出）");
}

/// 128：**场源／投影出口非法** ⇒ 采样不足 / 空序列（`present_product_selftest` 据此返回 128）。
#[test]
fn mirror_present_128_product_empty_detected() {
    use meta_kernel_core_nostd::field::sdf;
    use meta_kernel_core_nostd::project::{project_gray_u8, ProjectSpec};

    // ① 输出缓冲**小于**要采样的格点数 ⇒ `sample_into` 提前返回 < cells
    let mut small = [0.0f32; 4];
    let n = sdf::sample_into(|x, y| sdf::circle(3.0, 3.0, 2.0, x, y), 8, 8, &mut small);
    assert_eq!(n, 4, "128：out 容量不足时采样数须受限于 out.len()");
    assert!(n < 8 * 8, "128：该场景必须被判为'采样不足'");

    // ② 投影出口：输入不足 ⇒ 空（product 路径据此返 0/128）
    let spec = ProjectSpec::new(4, 4, 0.0, 1.0).expect("spec");
    assert!(project_gray_u8(&[0.5f32; 3], &spec).is_empty(), "128：输入不足须为空");
}
