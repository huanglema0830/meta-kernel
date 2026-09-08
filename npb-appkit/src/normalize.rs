//! # 事件归一（纯逻辑，无 IO）—— wasm/native 通用
//!
//! L3 SSE `state_change`/`instruction` → [`KernelEvent`]（方向规则 L4 §3.3）与原样载荷
//! [`RawEvent`]（供 Speaker/Journal 溯源）。纯函数，供 npb-appkit 各宿主与 wasm 前端复用。

use crate::lifecycle::KernelEvent;

pub struct RawEvent {
    /// SSE event 类型：state_change | instruction | snapshot | ping
    pub kind: String,
    /// data 载荷（instruction 为指令 JSON；state_change 为 {field,from,to} JSON）
    pub data: String,
    /// 归一的 KernelEvent（snapshot/ping 为 Hold 占位且不推进，见 `is_action`）。
    pub kernel: KernelEvent,
    /// 溯源句柄：{kind}#{首个字段值截断}
    pub source: String,
}

impl RawEvent {
    /// snapshot/ping 不算动作（EventPipe 消费侧过滤）。
    pub fn is_action(&self) -> bool {
        matches!(self.kind.as_str(), "state_change" | "instruction")
    }
}

fn json_field(json: &str, key: &str) -> Option<String> {
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

fn code_of(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}

/// 归一单条原始事件。
pub fn normalize(kind: &str, data: &str) -> KernelEvent {
    match kind {
        "state_change" => {
            let field = json_field(data, "field").unwrap_or_default();
            if field == "low_energy" {
                return if json_field(data, "to").as_deref() == Some("1") { KernelEvent::Settle } else { KernelEvent::Awaken };
            }
            let from = json_field(data, "from").and_then(|v| code_of(&v));
            let to = json_field(data, "to").and_then(|v| code_of(&v));
            match (from, to) {
                (Some(f), Some(t)) if t < f => KernelEvent::Awaken,
                (Some(f), Some(t)) if t > f => KernelEvent::Settle,
                _ => KernelEvent::Hold,
            }
        }
        "instruction" => {
            let ty = json_field(data, "type").unwrap_or_default();
            match ty.as_str() {
                "StateChanged" | "state_changed" => {
                    let from = json_field(data, "from").and_then(|v| code_of(&v));
                    let to = json_field(data, "to").and_then(|v| code_of(&v));
                    match (from, to) {
                        (Some(f), Some(t)) if t < f => KernelEvent::Awaken,
                        (Some(f), Some(t)) if t > f => KernelEvent::Settle,
                        _ => KernelEvent::Hold,
                    }
                }
                "LowEnergy" | "low_energy" => KernelEvent::Settle,
                "HabitFormed" | "habit_formed" => KernelEvent::Settle,
                "CompoundProduced" | "compound_produced"
                | "ResonanceFound" | "resonance_found"
                | "SelfIntensity" | "self_intensity" => KernelEvent::Awaken,
                _ => KernelEvent::Hold,
            }
        }
        _ => KernelEvent::Hold,
    }
}

pub(crate) fn make_source(kind: &str, data: &str) -> String {
    // 溯源句柄：类型 + 首个可见字段片段（截断 24）
    let frag = data.trim().chars().take(24).collect::<String>();
    format!("{kind}#{frag}")
}

