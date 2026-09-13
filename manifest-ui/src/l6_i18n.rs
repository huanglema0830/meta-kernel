//! L6 · 语言自动适配（l6_i18n）。
//!
//! - UI 文案：zh / en 两套（按 navigator.language 自动，可手动覆盖并记忆）；
//! - 诊断正文：从 L5 schema2 JSON 的 `summary.<lang>` 取用——
//!   `zh*` → `user`（口语最贴切，可手切 `tcm`）；`en*` → `universal`；其余回退 `universal`。
//! 零依赖：纯字符串映射（前端渲染用）。

/// 诊断语言键（summary 字段）。
pub fn summary_key_for(user_lang: &str, override_key: &str) -> String {
    if !override_key.is_empty() && override_key != "auto" {
        return override_key.to_string();
    }
    let l = user_lang.to_lowercase();
    if l.starts_with("zh") {
        "user".to_string()
    } else if l.starts_with("en") {
        "universal".to_string()
    } else {
        "universal".to_string()
    }
}

/// 语言显示名（UI 提示用）。
pub fn lang_label(key: &str) -> &'static str {
    match key {
        "user" => "中文（口语）",
        "tcm" => "中医",
        "universal" => "通用/English",
        "hardware" => "硬件",
        "software" => "软件",
        _ => "通用",
    }
}

/// UI 文案（zh / en）。
pub fn ui(lang_is_zh: bool, key: &str) -> String {
    let s = if lang_is_zh {
        match key {
            "no_data" => "尚未收到采集。请在目标设备打开 run-probe.bat 下载并双击，本页将自动呈现结论。",
            "refreshing" => "刷新中…",
            "calm" => "场域在节律内",
            "confidence" => "确信度",
            "advice" => "建议",
            "trace" => "溯源",
            "grown" => "内核生长",
            _ => key,
        }
    } else {
        match key {
            "no_data" => "No reading yet. On the target device download and run run-probe.bat; results appear here automatically.",
            "refreshing" => "Refreshing…",
            "calm" => "Field within rhythm",
            "confidence" => "Confidence",
            "advice" => "Advice",
            "trace" => "Trace",
            "grown" => "Kernel growth",
            _ => key,
        }
    };
    s.to_string()
}

/// 语气等级（菩萨戒·语气随确信度与偏离自动映射；此处给出 L6 侧统一口径）：
/// - 高确信（≥0.8）→ 直述；中（≥0.6）→ 陈述；低 → 谨慎（"可能"）。
pub fn tone_prefix(confidence: f64, zh: bool) -> &'static str {
    if confidence >= 0.8 {
        if zh { "" } else { "" }
    } else if confidence >= 0.6 {
        if zh { "" } else { "" }
    } else if zh {
        "可能："
    } else {
        "Possibly: "
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zh_maps_to_user() {
        assert_eq!(summary_key_for("zh-CN", "auto"), "user");
        assert_eq!(summary_key_for("zh-TW", ""), "user");
    }

    #[test]
    fn en_maps_to_universal() {
        assert_eq!(summary_key_for("en-US", "auto"), "universal");
        assert_eq!(summary_key_for("fr-FR", "auto"), "universal", "其它语言回退通用");
    }

    #[test]
    fn manual_override_wins() {
        assert_eq!(summary_key_for("zh-CN", "tcm"), "tcm");
        assert_eq!(summary_key_for("en-US", "hardware"), "hardware");
    }

    #[test]
    fn tone_by_confidence() {
        assert_eq!(tone_prefix(0.9, true), "");
        assert_eq!(tone_prefix(0.55, true), "可能：");
        assert_eq!(tone_prefix(0.55, false), "Possibly: ");
    }
}
