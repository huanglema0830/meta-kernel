//! L5 · 零依赖 JSON 输出给 L6（l5_router）。
//!
//! 输出结构（schema 2）含多语言 summary：L6 按用户语言自动匹配呈现，无需手动切换。
//! 手写 JSON 序列化（转义引号/反斜杠）；纯函数、即算即弃（不缓存）。

//! 【2.3b 片8 迁移】与 `meta-kernel-core/src/l5_router.rs` **同源**；仅作下述适配，其余**逐行逐字未改**：
//! ① 补 `alloc` 的 `use`（no_std 下 `format!`／`Vec`／`String`／`ToString` 不在 prelude）
//! ② 无 `std::` 路径需改写（本文件零 `std::` 用法）
//! ③ **本片零「替换类」**：无集合、无浮点方法、无 `std::` 常量 ⇒ 无类型/文字替换
//! ④ 本片清单**由 `--emit 2` 产出**（D40），**不手写**
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use crate::l5_compare::Band;
use crate::l5_diagnosis::Diagnosis;
use crate::l5_translate::all_summaries;

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn band_code(b: Band) -> &'static str {
    b.code()
}

/// 可选数值 → JSON（`None` 输出 `null`，不写成 0——"缺"与"零"必须能区分）。
fn opt_num(v: Option<f64>) -> String {
    match v {
        Some(x) if x.is_finite() => format!("{x}"),
        _ => "null".to_string(),
    }
}

/// 确信度依据 → JSON 对象（无证据 → `null`）。
fn basis_json(b: &Option<crate::l5_evidence::Adjustment>) -> String {
    match b {
        None => "null".to_string(),
        Some(a) => format!(
            "{{\"base\":{},\"world_match\":{},\"world_adjust\":{},\"prediction_error\":{},\"error_score\":{},\"pe_adjust\":{},\"final_confidence\":{},\"note\":\"{}\"}}",
            a.base,
            opt_num(a.world_match),
            a.world_adjust,
            opt_num(a.prediction_error),
            opt_num(a.error_score),
            a.pe_adjust,
            a.final_confidence,
            esc(&a.note)
        ),
    }
}

/// Diagnosis → JSON 字符串（schema 2；含 summary.{universal,hardware,software,plant,
/// animal,geology,tcm,user} 多语言字段——L6 按用户语言自动选）。
///
/// v0.123：`conclusion.basis` 输出**确信度依据**（世界模型匹配度 / 预测误差各自的修正量）。
/// **无证据时输出 `null`**——不臆造、不把"没有说话"伪装成"说了话"。
pub fn to_json(d: &Diagnosis) -> String {
    let names = crate::l5_senses::FIELDS;
    let pattern = (0..4)
        .map(|i| format!("\"{}\":\"{}\"", names[i], band_code(d.pattern[i])))
        .collect::<Vec<_>>()
        .join(",");
    let fields = (0..4)
        .map(|i| format!("\"{}\":{}", names[i], d.fields[i]))
        .collect::<Vec<_>>()
        .join(",");
    let sums = all_summaries(d)
        .iter()
        .map(|(k, v)| format!("\"{}\":\"{}\"", k, esc(v)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":{},\"fields\":{{{}}},\"pattern\":{{{}}},\"conclusion\":{{\"title\":\"{}\",\"description\":\"{}\",\"cause\":\"{}\",\"confidence\":{},\"suggestion_key\":\"{}\",\"suggestion\":\"{}\",\"basis\":{}}},\"trace\":{{\"baseline_id\":\"{}\",\"object\":\"{}\",\"at\":\"{}\",\"reproducible\":{}}},\"summary\":{{{}}}}}",
        d.schema,
        fields,
        pattern,
        esc(&d.conclusion.title),
        esc(&d.conclusion.description),
        esc(&d.conclusion.cause),
        d.conclusion.confidence,
        esc(&d.conclusion.suggestion_key),
        esc(&d.conclusion.suggestion),
        // **结论携带的依据**（世界模型匹配度 + 预测误差）——L6 可据此解释"为何这个确信度"
        basis_json(&d.conclusion.basis),
        esc(&d.trace.baseline_id),
        esc(&d.trace.object),
        esc(&d.trace.at),
        if d.trace.reproducible { "true" } else { "false" },
        sums
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l5_baseline::BaselineField;
    use crate::l5_diagnosis::{diagnose, Diagnosis};

    fn base() -> BaselineField {
        BaselineField { earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0, object: "n", established: "learned" }
    }
    fn sample() -> Diagnosis {
        diagnose(&[2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0], &base(), "2015-notebook")
    }

    #[test]
    fn json_contains_all_fields_and_multi_lang_summary() {
        let j = to_json(&sample());
        assert!(j.starts_with("{\"schema\":2"), "{j}");
        for k in ["fields", "pattern", "conclusion", "trace", "summary"] {
            assert!(j.contains(&format!("\"{k}\"")), "缺 {k}");
        }
        for lang in crate::l5_translate::LANGS {
            assert!(j.contains(&format!("\"{lang}\":")), "缺 summary.{lang}");
        }
        assert!(j.contains("\"fire\":\"Kang\""), "pattern fire=Kang: {j}");
        assert!(j.contains("\"water\":\"Ku\""));
    }

    #[test]
    fn json_quotes_escaped() {
        // cause 含中文引号场景：构造含引号文本验证不破坏 JSON
        let mut d = sample();
        d.conclusion.title = "标题含\"引号\"".to_string();
        let j = to_json(&d);
        assert!(j.contains("\\\"引号\\\""), "{j}");
    }

    #[test]
    fn calm_json_has_summary_user() {
        let d = diagnose(&[1.0; 7], &base(), "obj");
        let j = to_json(&d);
        assert!(j.contains("\"user\":\"一切正常\""), "{j}");
    }

    #[test]
    fn json_repeatable_and_no_residue() {
        let d = sample();
        let a = to_json(&d);
        for _ in 0..3 {
            assert_eq!(to_json(&d), a);
        }
    }

    #[test]
    fn basis_is_null_without_evidence_and_present_with_it() {
        use crate::l5_baseline::BaselineField;
        use crate::l5_evidence::{Evidence, Gains};
        use crate::l5_diagnosis::diagnose_with_evidence;

        let b = BaselineField { earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0, object: "o", established: "learned" };
        let s = [1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0];

        // 无证据 → null（不臆造）
        let d0 = diagnose(&s, &b, "obj");
        let j0 = to_json(&d0);
        assert!(j0.contains("\"basis\":null"), "无证据必须输出 null：{j0}");

        // 有证据 → 携带原值与修正量（发起人要求：结论携带预测误差作为确信度依据）
        let d1 = diagnose_with_evidence(
            &s,
            &b,
            "obj",
            &Evidence { world_match: Some(0.9), prediction_error: Some(0.003) },
            &Gains::default(),
        );
        let j1 = to_json(&d1);
        assert!(j1.contains("\"prediction_error\":0.003"), "须带误差原值：{j1}");
        assert!(j1.contains("\"world_match\":0.9"), "须带匹配度：{j1}");
        assert!(j1.contains("\"error_score\":"), "须带归一化分：{j1}");
        assert!(!j1.contains("basis\":null"), "有证据不得输出 null");
        // 依据的修正必须与最终确信度自洽（final = base + world + pe，且被 0..1 夹住）
        assert!(j1.contains("\"final_confidence\":"), "{j1}");
    }
}
