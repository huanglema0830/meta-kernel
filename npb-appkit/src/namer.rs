//! # 命名 registry（L4 §4.1 双轨表；条目预留 twin 字段位）

/// 命名实体：内部符号 ↔ 外部名（只增不改；跨会话可复用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedEntity {
    pub internal: &'static str,
    pub external: &'static str,
    /// 预留 twin 字段位（v1.0 发起人补充）：二期孪生指纹（twin_fingerprint=!fp）
    /// 经此位挂载；**一期恒 None，不占用实现**。
    pub twin: Option<u64>,
}

/// 圈层带（十位 1..9）语义名（L4 §3.2）。
const BANDS: [(&str, &str); 9] = [
    ("1", "萌发带 Awakening"),
    ("2", "涌动带 Surging"),
    ("3", "凝结带 Condensing"),
    ("4", "汇聚带 Converging"),
    ("5", "流淌带 Flowing"),
    ("6", "塑形带 Molding"),
    ("7", "结晶带 Crystallizing"),
    ("8", "固化带 Solidifying"),
    ("9", "极显带 Manifest"),
];

/// 命名器：给定生命周期状态/内部符号取外部名。
pub struct Namer;

impl Namer {
    /// 生命周期状态的外部名：0 → "锚点 Void-Awake"；10..99 → "圈层名·第 N 步"；99 加圆满注记。
    pub fn band_of(state: u16) -> String {
        if state == 0 {
            return "锚点 Void-Awake".to_string();
        }
        let band = (state / 10) as usize;
        let step = state % 10;
        let name = BANDS[band - 1].1;
        if state == 99 {
            format!("{name} · 圆满")
        } else {
            format!("{name} · 第{step}步")
        }
    }

    /// 内部符号 → 命名实体（物态/层）。
    pub fn entity(internal: &'static str) -> Option<NamedEntity> {
        let external = match internal {
            "state.flow" | "state.budget" => return None, // 取值态名走 state_of
            "lifecycle" => return None,                   // 动态名走 band_of
            "instruction.state_changed" => "内核转向",
            "instruction.self_intensity" => "自我感",
            "instruction.low_energy" => "低能量预警",
            "instruction.compound_produced" => "化合生成",
            "instruction.habit_formed" => "习气成形",
            "instruction.resonance_found" => "共振达成",
            "element.wind" => "风 Wind",
            "element.fire" => "火 Fire",
            "element.water" => "水 Water",
            "element.earth" => "地 Earth",
            _ => return None,
        };
        Some(NamedEntity { internal, external, twin: None })
    }

    /// 物态代码外部名（flow/budget 0..3 共用同一词表）。
    pub fn state_of(code: u32) -> &'static str {
        match code {
            0 => "能量态 Energy",
            1 => "气态 Gas",
            2 => "液态 Liquid",
            _ => "固态 Solid",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_names_cover_all_bands_and_anchor() {
        assert_eq!(Namer::band_of(0), "锚点 Void-Awake");
        assert_eq!(Namer::band_of(10), "萌发带 Awakening · 第0步");
        assert_eq!(Namer::band_of(19), "萌发带 Awakening · 第9步");
        assert_eq!(Namer::band_of(55), "流淌带 Flowing · 第5步");
        assert_eq!(Namer::band_of(99), "极显带 Manifest · 圆满");
    }

    #[test]
    fn entities_expose_twin_slot_empty() {
        let e = Namer::entity("instruction.resonance_found").expect("exists");
        assert_eq!(e.external, "共振达成");
        assert!(e.twin.is_none(), "一期 twin 恒空");
        assert!(Namer::entity("nope").is_none());
    }

    #[test]
    fn states_named_consistently() {
        assert_eq!(Namer::state_of(0), "能量态 Energy");
        assert_eq!(Namer::state_of(3), "固态 Solid");
    }
}
