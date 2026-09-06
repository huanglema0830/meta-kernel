//! # 指令发布器（Executor）— 思流照亮
//!
//! 把"心流凿空"的结果与状态变化转化为对外指令，指挥宿主执行。
//! 指令可序列化为 JSON（零第三方依赖，手写序列化），经 NPB 桥接器发送到外部。

use crate::state::State;
use crate::state::{GOLDEN_RATIO, GOLDEN_THIRD};

/// 共振阈值：化合产物越过黄金分割即视为"共振达成"（心流凿空的结果）。
pub const RESONANCE_THRESHOLD: f32 = GOLDEN_RATIO;
/// 低能量阈值：储备低于此值（液态下界）触发 LowEnergy。
pub const LOW_ENERGY_THRESHOLD: f32 = GOLDEN_THIRD;
/// 习气形成阈值（强度）。
pub const HABIT_FORM_THRESHOLD: f32 = 0.5;
/// 化合产物发布阈值。
pub const COMPOUND_THRESHOLD: f32 = 0.3;
/// 自我感变化触发阈值（绝对差）。
pub const SELF_INTENSITY_DELTA: f32 = 0.1;

/// 内核对外指令。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KernelInstruction {
    /// 物态变化（from → to）。
    StateChanged { from: State, to: State },
    /// 自我感强度（0-1）。
    SelfIntensity { level: f32 },
    /// 低能量（储备枯竭预警），stored 为当前储备。
    LowEnergy { stored: f32 },
    /// 化合产物生成，product 为产物能量。
    CompoundProduced { product: f32 },
    /// 习气形成，fingerprint 为模式指纹，strength 为习气强度。
    HabitFormed { fingerprint: u64, strength: f32 },
    /// 共振达成（心流凿空的结果），twin_fingerprint 为命中的孪生指纹。
    ResonanceFound { twin_fingerprint: u64 },
}

impl KernelInstruction {
    /// 指令类型名（JSON 的 "type" 字段）。
    pub const fn kind(&self) -> &'static str {
        match self {
            KernelInstruction::StateChanged { .. } => "StateChanged",
            KernelInstruction::SelfIntensity { .. } => "SelfIntensity",
            KernelInstruction::LowEnergy { .. } => "LowEnergy",
            KernelInstruction::CompoundProduced { .. } => "CompoundProduced",
            KernelInstruction::HabitFormed { .. } => "HabitFormed",
            KernelInstruction::ResonanceFound { .. } => "ResonanceFound",
        }
    }

    /// 手写 JSON 序列化（UTF-8，状态用中文标签，零依赖、可解析）。
    pub fn to_json(&self) -> String {
        let jf = |v: f32| format!("{:.4}", v);
        match self {
            KernelInstruction::StateChanged { from, to } => format!(
                "{{\"type\":\"StateChanged\",\"from\":\"{}\",\"to\":\"{}\"}}",
                from.label_cn(),
                to.label_cn()
            ),
            KernelInstruction::SelfIntensity { level } => {
                format!("{{\"type\":\"SelfIntensity\",\"level\":{}}}", jf(*level))
            }
            KernelInstruction::LowEnergy { stored } => {
                format!("{{\"type\":\"LowEnergy\",\"stored\":{}}}", jf(*stored))
            }
            KernelInstruction::CompoundProduced { product } => {
                format!("{{\"type\":\"CompoundProduced\",\"product\":{}}}", jf(*product))
            }
            KernelInstruction::HabitFormed { fingerprint, strength } => format!(
                "{{\"type\":\"HabitFormed\",\"fingerprint\":{},\"strength\":{}}}",
                fingerprint,
                jf(*strength)
            ),
            KernelInstruction::ResonanceFound { twin_fingerprint } => format!(
                "{{\"type\":\"ResonanceFound\",\"twin_fingerprint\":{}}}",
                twin_fingerprint
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;

    fn valid_json(s: &str) -> bool {
        s.starts_with('{') && s.ends_with('}') && s.contains("\"type\"")
    }

    #[test]
    fn json_contains_type_and_fields() {
        let s = KernelInstruction::StateChanged {
            from: State::Energy,
            to: State::Solid,
        }
        .to_json();
        assert!(valid_json(&s), "{}", s);
        assert!(s.contains("\"type\":\"StateChanged\""));
        assert!(s.contains("\"from\":\"能量态\""));
        assert!(s.contains("\"to\":\"固态\""));

        let r = KernelInstruction::ResonanceFound {
            twin_fingerprint: 0xABCD,
        }
        .to_json();
        assert!(r.contains("\"type\":\"ResonanceFound\""));
        assert!(r.contains("\"twin_fingerprint\":43981"));

        let h = KernelInstruction::HabitFormed {
            fingerprint: 7,
            strength: 0.8123,
        }
        .to_json();
        assert!(h.contains("\"fingerprint\":7"));
        assert!(h.contains("\"strength\":0.8123"));

        let l = KernelInstruction::LowEnergy { stored: 0.1234 }.to_json();
        assert!(l.contains("\"stored\":0.1234"));

        let c = KernelInstruction::CompoundProduced { product: 0.7 }.to_json();
        assert!(c.contains("\"product\":0.7000"));
    }

    #[test]
    fn thresholds_match_spec() {
        assert!((RESONANCE_THRESHOLD - 0.618_033_9).abs() < 1e-6);
        assert!((LOW_ENERGY_THRESHOLD - 0.206_011_3).abs() < 1e-6);
        assert!((HABIT_FORM_THRESHOLD - 0.5).abs() < 1e-6);
        assert!((COMPOUND_THRESHOLD - 0.3).abs() < 1e-6);
    }

    #[test]
    fn kind_name_matches_json_type() {
        let instr = KernelInstruction::SelfIntensity { level: 0.9 };
        assert_eq!(instr.kind(), "SelfIntensity");
        assert!(instr.to_json().contains(&format!("\"type\":\"{}\"", instr.kind())));
    }
}
