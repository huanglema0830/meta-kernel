//! L5 六模块集成验证（端到端链：采样 → 拆解 → 本底 → 对比 → 诊断 → 多语言 JSON）。
//! 覆盖 Q1–Q7 + L4 拒绝路径（被拒不进入号脉）。

use meta_kernel_core::l4::dimension::FieldState;
use meta_kernel_core::l4::l4_gate::RejectReason;
use meta_kernel_core::l4::l4_router::{route_state, route_passed};
use meta_kernel_core::l5_baseline::BaselineField;
use meta_kernel_core::l5_compare::Band;
use meta_kernel_core::l5_diagnosis::{diagnose, Diagnosis};
use meta_kernel_core::l5_router::to_json;

fn base() -> BaselineField {
    BaselineField { earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0, object: "test-notebook", established: "learned" }
}

fn l5(s: &[f64; 7], object: &str) -> Diagnosis {
    diagnose(s, &base(), object)
}

#[test]
fn q1_calm_end_to_end() {
    let s = [1.0; 7];
    let d = l5(&s, "obj");
    assert_eq!(d.pattern, [Band::Ping; 4]);
    let j = to_json(&d);
    assert!(j.contains("\"user\":\"一切正常\""), "{j}");
    // L6 可据语言自动取 summary.user / .universal
    assert!(j.contains("\"universal\":\"平稳运行（全场节律内）\""), "{j}");
}

#[test]
fn q2_fire_kang_end_to_end() {
    let s = [2.0, 1.0, 2.0, 1.0, 1.0, 1.0, 1.0]; // fire=(a+t)/2=2 → 亢
    let d = l5(&s, "obj");
    assert_eq!(d.pattern[2], Band::Kang);
    let j = to_json(&d);
    assert!(j.contains("\"fire\":\"Kang\""), "{j}");
}

#[test]
fn q3_water_ku_end_to_end() {
    let s = [1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0]; // entropy=0.5 → 枯
    let d = l5(&s, "obj");
    assert_eq!(d.pattern[1], Band::Ku);
}

#[test]
fn q4_composite_single_conclusion() {
    let s = [2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0];
    let d = l5(&s, "obj");
    // 一因一果：title 单主导；desc 注明水不济火；summary.tcm 有中医桥
    let j = to_json(&d);
    assert!(d.conclusion.title.contains("fire"));
    assert!(j.contains("水不济火"), "{j}");
    assert!(j.contains("\"tcm\":"));
}

#[test]
fn q5_baseline_learn_stable_roundtrip() {
    // 学习 → 编码 → 解码 → 用恢复的本底场再判定一致
    let samples = [[1.0; 4], [1.1, 0.9, 1.0, 1.0], [0.9, 1.1, 1.0, 1.0]];
    let learned = BaselineField::learn(&samples, "obj");
    let j = learned.to_json();
    let restored = BaselineField::from_json(&j).expect("可恢复");
    // 同采样下 learned 与 restored 判定应一致（earth≈1.0 等）
    let s = [2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0];
    let d1 = l5_with(&s, &learned);
    let d2 = l5_with(&s, &restored);
    assert_eq!(d1.pattern, d2.pattern, "恢复本底场判定一致");
}

fn l5_with(s: &[f64; 7], b: &BaselineField) -> Diagnosis {
    let fields = meta_kernel_core::l5_senses::decompose(s);
    let pattern = meta_kernel_core::l5_compare::compare(&fields, b);
    let conclusion = meta_kernel_core::l5_diagnosis::synthesize(&fields, b, &pattern);
    Diagnosis {
        schema: 2,
        fields,
        pattern,
        conclusion,
        trace: meta_kernel_core::l5_diagnosis::Traceability {
            baseline_id: "b-test".into(), object: "obj".into(), at: "t".into(), reproducible: true,
        },
    }
}

#[test]
fn q6_no_residue_repeatable() {
    let s = [2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0];
    let a = to_json(&l5(&s, "obj"));
    for _ in 0..5 {
        assert_eq!(to_json(&l5(&s, "obj")), a, "重复号脉结果恒定（无缓存残留）");
    }
}

#[test]
fn q7_multi_lang_summary_and_pending_marker() {
    let s = [2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0];
    let j = to_json(&l5(&s, "obj"));
    for lang in meta_kernel_core::l5_translate::LANGS {
        assert!(j.contains(&format!("\"{lang}\":")), "summary.{lang} 存在");
    }
    // software 部分词缺译 → (待补) 不阻断
    assert!(j.contains("(待补)") || j.contains("software"), "缺词回退不阻断: {j}");
}

#[test]
fn l4_rejected_path_has_no_l5_payload() {
    // L4 拒绝（不非时食）→ 无 L5Payload → 号脉不启动（拒绝即弃）
    let bad = FieldState::new(2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);
    let b = FieldState::baseline();
    let r = route_state(&bad, &b);
    assert_eq!(r, Err(RejectReason::UnseasonalMeal(3)));
    // 通过路径才有 L5Payload（l4_router 预留）
    let good = FieldState::new(1.0, 1.0, 0.5, 1.0, 1.0, 1.0, 1.0);
    if let Ok(v) = route_state(&good, &b) {
        let payload = route_passed(&v);
        assert_eq!(payload.state.len(), 7);
        assert_eq!(payload.extra, None);
    } else {
        panic!("通过态应放行");
    }
}

/// 报告输出演示（L5_VALIDATION_REPORT 数据源；nocapture 运行查看 JSON）。
#[test]
fn demo_report_outputs() {
    let qs = [
        ("Q1 平稳", [1.0; 7]),
        ("Q2 火亢", [2.0, 1.0, 2.0, 1.0, 1.0, 1.0, 1.0]),
        ("Q3 水枯", [1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0]),
        ("Q4 火亢+水枯", [2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0]),
    ];
    for (name, s) in qs {
        let d = l5(&s, "2015-notebook");
        let names = ["earth", "water", "fire", "wind"];
        let pat: Vec<String> = (0..4).map(|i| format!("{}:{}", names[i], d.pattern[i].code())).collect();
        println!("== {name} ==");
        println!("fields [{:.2}, {:.2}, {:.2}, {:.2}]", d.fields[0], d.fields[1], d.fields[2], d.fields[3]);
        println!("pattern {{{}}}", pat.join(", "));
        println!("title: {}", d.conclusion.title);
        println!("cause: {}", d.conclusion.cause);
        println!("summary.user: {}", summary_for("user", &d));
        println!("summary.tcm:  {}", summary_for("tcm", &d));
    }
    let d4 = l5(&[2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0], "2015-notebook");
    println!("== Q4 full JSON ==");
    println!("{}", to_json(&d4));
}

fn summary_for(lang: &str, d: &Diagnosis) -> String {
    meta_kernel_core::l5_translate::summarize_for(meta_kernel_core::l5_translate::TERMS, lang, d)
}
