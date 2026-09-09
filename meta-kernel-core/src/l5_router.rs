//! L5 · 零依赖 JSON 输出给 L6（l5_router）。
//!
//! 输出结构（schema 2）含多语言 summary：L6 按用户语言自动匹配呈现，无需手动切换。
//! 手写 JSON 序列化（转义引号/反斜杠）；纯函数、即算即弃（不缓存）。

use crate::l5_compare::Band;
use crate::l5_diagnosis::Diagnosis;
use crate::l5_translate::all_summaries;

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn band_code(b: Band) -> &'static str {
    b.code()
}

/// Diagnosis → JSON 字符串（schema 2；含 summary.{universal,hardware,software,plant,
/// animal,geology,tcm,user} 多语言字段——L6 按用户语言自动选）。
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
        "{{\"schema\":{},\"fields\":{{{}}},\"pattern\":{{{}}},\"conclusion\":{{\"title\":\"{}\",\"description\":\"{}\",\"cause\":\"{}\"}},\"trace\":{{\"baseline_id\":\"{}\",\"object\":\"{}\",\"at\":\"{}\",\"reproducible\":{}}},\"summary\":{{{}}}}}",
        d.schema,
        fields,
        pattern,
        esc(&d.conclusion.title),
        esc(&d.conclusion.description),
        esc(&d.conclusion.cause),
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
}
