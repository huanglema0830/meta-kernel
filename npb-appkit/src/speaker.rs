//! # 语言组织（L4 §4.2：指称 → 陈述 → 意图 三层流水线）
//!
//! 戒律：每条陈述/意图输出必带 `source`（可溯源到指令/状态字段）；只翻译不增义。
//! 模板中的 {x} 由调用方填字段直值；本层不做任何推断。

use crate::namer::Namer;

/// 可溯源陈述（输出层最小单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    pub text: String,
    /// 溯源：e.g. "instruction#compound_produced|state_change#low_energy"
    pub source: String,
}

/// 意图建议（一期基础原语：suggest_next / journal）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    pub kind: &'static str,
    pub text: String,
    pub source: String,
}

/// Speaker：陈述与意图模板（一期固定词表，随 registry 版本扩展）。
pub struct Speaker;

/// 内核指令 JSON 的字段提取（极简，仅取模板所需）。
fn field_of(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let idx = json.find(&needle)?;
    let rest = &json[idx + needle.len()..];
    let colon = rest.find(':')? + 1;
    let val: String = rest[colon..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+' || *c == '"' || c.is_alphabetic())
        .collect();
    if val.is_empty() { None } else { Some(val.trim_matches('"').to_string()) }
}

impl Speaker {
    /// 陈述层：指令 JSON → 一句可溯源陈述（六型模板；未知类型返回 None）。
    pub fn statement_of_instruction(json: &str) -> Option<Statement> {
        let kind = field_of(json, "type")?;
        let src = format!("instruction#{kind}");
        let text = match kind.as_str() {
            "state_changed" | "StateChanged" => {
                let from = field_of(json, "from").unwrap_or_default();
                let to = field_of(json, "to").unwrap_or_default();
                format!("内核正从{}转向{}", Namer::state_of(from.parse().unwrap_or(0)), Namer::state_of(to.parse().unwrap_or(0)))
            }
            "self_intensity" | "SelfIntensity" => {
                format!("自我感升至 {}", field_of(json, "level").unwrap_or_default())
            }
            "low_energy" | "LowEnergy" => {
                format!("能量储备触及低位（{}），显化进程放缓", field_of(json, "stored").unwrap_or_default())
            }
            "compound_produced" | "CompoundProduced" => {
                format!("化合生成：创新增量 {}", field_of(json, "amount").unwrap_or_default())
            }
            "habit_formed" | "HabitFormed" => {
                format!("习气成形：模式 #{}，强度 {}", field_of(json, "fingerprint").unwrap_or_default(), field_of(json, "strength").unwrap_or_default())
            }
            "resonance_found" | "ResonanceFound" => {
                format!("共振达成：命中孪生模式 #{}", field_of(json, "twin_fingerprint").unwrap_or_default())
            }
            _ => return None,
        };
        Some(Statement { text, source: src })
    }

    /// 陈述层：生命周期转移 → 陈述。
    pub fn statement_of_lifecycle(from: u16, to: u16) -> Statement {
        let text = if to == 0 {
            if from == 99 {
                "回融：本轮圆满，回归 0 锚点".to_string()
            } else {
                format!("早退回融：由 {} 回归 0 锚点", Namer::band_of(from))
            }
        } else {
            format!("显化推进：{} → {}", Namer::band_of(from), Namer::band_of(to))
        };
        Statement { text, source: "lifecycle_engine".to_string() }
    }

    /// 意图层：一期基础原语（suggest_next / journal）。全部输出可溯源。
    pub fn intent_of(kind: &'static str, st: &Statement, lifecycle: u16, stored_low: bool) -> Intent {
        let text = match kind {
            crate::INTENT_SUGGEST_NEXT => {
                if stored_low {
                    format!("储备尚足度低：建议注入一次补充扰动以延续显化（当前 {}）", Namer::band_of(lifecycle))
                } else {
                    format!("可再注入一条补充扰动以延续显化（当前 {}）", Namer::band_of(lifecycle))
                }
            }
            crate::INTENT_JOURNAL => format!("已记录：{}", st.text),
            _ => "".to_string(),
        };
        Intent { kind, text, source: st.source.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_statements_are_traceable() {
        let s = Speaker::statement_of_instruction(r#"{"type":"StateChanged","from":0,"to":2}"#).unwrap();
        assert_eq!(s.text, "内核正从能量态 Energy转向液态 Liquid");
        assert_eq!(s.source, "instruction#StateChanged");

        let l = Speaker::statement_of_instruction(r#"{"type":"LowEnergy","stored":0.12}"#).unwrap();
        assert!(l.text.contains("0.12"), "{}", l.text);
        assert_eq!(l.source, "instruction#LowEnergy");

        assert!(Speaker::statement_of_instruction(r#"{"type":"???"}"#).is_none());
    }

    #[test]
    fn lifecycle_statements_cover_retreat() {
        assert_eq!(Speaker::statement_of_lifecycle(99, 0).text, "回融：本轮圆满，回归 0 锚点");
        assert_eq!(Speaker::statement_of_lifecycle(10, 11).text, "显化推进：萌发带 Awakening · 第0步 → 萌发带 Awakening · 第1步");
        assert!(Speaker::statement_of_lifecycle(10, 11).source.contains("lifecycle"));
    }

    #[test]
    fn intents_carry_source() {
        let st = Speaker::statement_of_lifecycle(10, 11);
        let it = Speaker::intent_of(crate::INTENT_SUGGEST_NEXT, &st, 11, false);
        assert_eq!(it.kind, crate::INTENT_SUGGEST_NEXT);
        assert!(it.text.contains("萌发带"));
        assert_eq!(it.source, st.source);
    }

    #[test]
    fn intent_flags_low_stored() {
        let st = Speaker::statement_of_lifecycle(20, 21);
        let it = Speaker::intent_of(crate::INTENT_SUGGEST_NEXT, &st, 21, true);
        assert!(it.text.contains("储备尚足度低"), "{}", it.text);
    }
}
