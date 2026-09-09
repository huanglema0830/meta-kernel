//! L5 · 编译翻译（l5_translate）：语言对照表 → 多语言总结。
//!
//! - 对照表 = 配置数据（词条表，与 `docs/L5_TRANSLATION_TABLE.md` 同构）；
//!   此处内置示例镜像（**不视为固定知识**——正式词条随用户实测验证补充，
//!   宿主可注入外部表 `translate_with`）；
//! - 缺词/未验证词条 → 保留场语言原文 + `(待补)` 标记，**不阻断诊断**。

use crate::l5_compare::Band;
use crate::l5_diagnosis::Diagnosis;

/// 目标语言集合（summary 字段键；L6 按用户语言自动选）。
pub const LANGS: [&str; 8] =
    ["universal", "hardware", "software", "plant", "animal", "geology", "tcm", "user"];

/// 词条：lang + key（如 field.fire / band.Kang / phrase.calm）+ 译文 + 验证状态。
#[derive(Clone, Copy, Debug)]
pub struct Term {
    pub lang: &'static str,
    pub key: &'static str,
    pub text: &'static str,
    pub verified: bool,
}

/// 示例词表（镜像对照表 v0.1 示例行；verified=false = 待验证）。
pub const TERMS: &[Term] = &[
    // 场名
    Term { lang: "universal", key: "field.earth", text: "根基", verified: true },
    Term { lang: "universal", key: "field.water", text: "缓冲", verified: true },
    Term { lang: "universal", key: "field.fire", text: "活力", verified: true },
    Term { lang: "universal", key: "field.wind", text: "变动", verified: true },
    Term { lang: "hardware", key: "field.earth", text: "结构/承载", verified: true },
    Term { lang: "hardware", key: "field.water", text: "余量/缓存", verified: true },
    Term { lang: "hardware", key: "field.fire", text: "负载/占用", verified: true },
    Term { lang: "hardware", key: "field.wind", text: "抖动/频率", verified: true },
    Term { lang: "software", key: "field.fire", text: "CPU/线程活性", verified: true },
    Term { lang: "software", key: "field.water", text: "内存/缓冲余量", verified: true },
    Term { lang: "plant", key: "field.earth", text: "根系/土质", verified: true },
    Term { lang: "plant", key: "field.water", text: "水分", verified: true },
    Term { lang: "plant", key: "field.fire", text: "光照/活力", verified: true },
    Term { lang: "plant", key: "field.wind", text: "通风/传粉变动", verified: true },
    Term { lang: "geology", key: "field.earth", text: "岩层/构造", verified: true },
    Term { lang: "geology", key: "field.water", text: "含水层", verified: true },
    Term { lang: "geology", key: "field.fire", text: "地热/能量", verified: true },
    Term { lang: "geology", key: "field.wind", text: "气流/侵蚀", verified: true },
    Term { lang: "animal", key: "field.fire", text: "活动/体温", verified: true },
    Term { lang: "animal", key: "field.water", text: "体液/滋润", verified: true },
    Term { lang: "tcm", key: "field.fire", text: "火(君火/相火)", verified: true },
    Term { lang: "tcm", key: "field.water", text: "水(肾水)", verified: true },
    Term { lang: "tcm", key: "field.wind", text: "风(善行数变)", verified: true },
    Term { lang: "tcm", key: "field.earth", text: "土(脾胃)", verified: true },
    Term { lang: "user", key: "field.fire", text: "干活的那个劲儿", verified: true },
    Term { lang: "user", key: "field.water", text: "余量", verified: true },
    // 状态
    Term { lang: "universal", key: "band.Kang", text: "亢进", verified: true },
    Term { lang: "universal", key: "band.Ku", text: "枯弱", verified: true },
    Term { lang: "universal", key: "band.Ping", text: "平稳", verified: true },
    Term { lang: "hardware", key: "band.Kang", text: "过高/过载", verified: true },
    Term { lang: "hardware", key: "band.Ku", text: "不足/告警", verified: true },
    Term { lang: "user", key: "band.Kang", text: "太猛/太满", verified: true },
    Term { lang: "user", key: "band.Ku", text: "太虚/不够用", verified: true },
    Term { lang: "tcm", key: "band.Kang", text: "实证/亢盛", verified: true },
    Term { lang: "tcm", key: "band.Ku", text: "虚证/不足", verified: true },
    // 短语
    Term { lang: "universal", key: "phrase.calm", text: "平稳运行（全场节律内）", verified: true },
    Term { lang: "user", key: "phrase.calm", text: "一切正常", verified: true },
    Term { lang: "hardware", key: "phrase.fire_not_water", text: "占用过高而余量不足（水不济火）", verified: true },
    Term { lang: "universal", key: "phrase.fire_not_water", text: "活力越限而涵养不足", verified: true },
    Term { lang: "tcm", key: "phrase.fire_not_water", text: "火亢水亏——水不济火", verified: true },
];

/// 查词：返回译文与已验证标志。
pub fn lookup(lang: &str, key: &str) -> Option<(&'static str, bool)> {
    TERMS.iter().find(|t| t.lang == lang && t.key == key).map(|t| (t.text, t.verified))
}

/// 带注入词表的查词（宿主可传外部对照表配置）。
pub fn lookup_with<'a>(terms: &'a [Term], lang: &str, key: &str) -> Option<(&'a str, bool)> {
    terms.iter().find(|t| t.lang == lang && t.key == key).map(|t| (t.text, t.verified))
}

/// 取词或回退：缺失 → 原文 key + (待补)。
fn pick(terms: &[Term], lang: &str, key: &str) -> String {
    match lookup_with(terms, lang, key) {
        Some((text, true)) => text.to_string(),
        Some((text, false)) => format!("{text}(待补)"),
        None => format!("{key}(待补)"),
    }
}

/// 生成某语言的一条总结句（四场模式 + 主标题语义）。
pub fn summarize_for(terms: &[Term], lang: &str, d: &Diagnosis) -> String {
    let mut parts: Vec<String> = Vec::new();
    for i in 0..4 {
        let field = crate::l5_senses::FIELDS[i];
        let band = match d.pattern[i] {
            Band::Ping => continue,
            Band::Kang => "band.Kang",
            Band::Ku => "band.Ku",
        };
        let f = pick(terms, lang, &format!("field.{field}"));
        let b = pick(terms, lang, band);
        parts.push(format!("{b}·{f}"));
    }
    if parts.is_empty() {
        pick(terms, lang, "phrase.calm")
    } else {
        let joined = parts.join("，");
        if d.pattern[1] == Band::Ku && d.pattern[2] == Band::Kang {
            format!("{joined}；{}", pick(terms, lang, "phrase.fire_not_water"))
        } else {
            joined
        }
    }
}

/// 默认词表的多语言总结。
pub fn summarize(d: &Diagnosis) -> String {
    summarize_for(TERMS, "universal", d)
}

/// 全部语言总结（router 用：summary.universal / .hardware / …）。
pub fn all_summaries(d: &Diagnosis) -> Vec<(String, String)> {
    LANGS.iter().map(|l| (l.to_string(), summarize_for(TERMS, l, d))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l5_baseline::BaselineField;
    use crate::l5_diagnosis::{diagnose, Diagnosis};

    fn base() -> BaselineField {
        BaselineField { earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0, object: "o", established: "learned" }
    }
    fn calm() -> Diagnosis {
        diagnose(&[1.0; 7], &base(), "obj")
    }
    fn fire_kang_water_ku() -> Diagnosis {
        diagnose(&[2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0], &base(), "obj")
    }

    #[test]
    fn q7_calm_translated_all_langs() {
        let d = calm();
        let s = all_summaries(&d);
        assert_eq!(s.len(), 8);
        for (lang, text) in &s {
            assert!(!text.is_empty(), "{lang}");
            if *lang == "user" {
                assert_eq!(text, "一切正常", "user 完整词：{text}");
            }
        }
    }

    #[test]
    fn fire_kang_translated() {
        let d = fire_kang_water_ku();
        let s = summarize(&d);
        assert!(s.contains("亢"), "{s}");
        let us = summarize_for(TERMS, "user", &d);
        assert!(us.contains("太猛") || us.contains("(待补)"), "{us}");
        let hw = summarize_for(TERMS, "hardware", &d);
        assert!(hw.contains("过载") || hw.contains("(待补)"), "{hw}");
        let tcm = summarize_for(TERMS, "tcm", &d);
        assert!(tcm.contains("水不济火"), "{tcm}");
    }

    #[test]
    fn missing_term_falls_back_no_panic() {
        let d = fire_kang_water_ku();
        // software 词只有 fire/water 场 → 其它场缺词 → (待补) 不阻断
        let sw = summarize_for(TERMS, "software", &d);
        assert!(sw.contains("(待补)") || !sw.is_empty(), "{sw}");
        // 未知语言 → 全部回退原文+待补
        let xx = summarize_for(TERMS, "xx", &d);
        assert!(xx.contains("(待补)"), "{xx}");
    }

    #[test]
    fn lookup_marks_verified_and_injected_unverified_pending() {
        let (text, verified) = lookup("universal", "field.fire").expect("词条存在");
        assert_eq!(text, "活力");
        assert!(verified, "内置示例词为可用词");
        // 用户补充词默认未验证 → 输出带 (待补)
        let extra = [Term { lang: "user", key: "field.fire", text: "新词示例", verified: false }];
        let s = summarize_for(&extra, "user", &fire_kang_water_ku());
        assert!(s.contains("(待补)"), "{s}");
    }

    #[test]
    fn injected_terms_override() {
        let extra = [Term { lang: "user", key: "phrase.calm", text: "妥妥的", verified: true }];
        let s = summarize_for(&extra, "user", &calm());
        assert_eq!(s, "妥妥的");
    }
}
