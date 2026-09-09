//! L5 · 诊断结论生成（l5_diagnosis）——三量运算。
//!
//! 存量 = 本底场（对象自身地图）；变量 = 当前场偏移（亢/枯/平）；
//! 补充增量 = 成因（是什么导致偏移）。
//! 戒律落实：不邪淫（一次号脉单一主结论）；不妄语（trace 可复现）；
//! 不饮酒（只用自身场数据推导）。纯函数、无状态。

use crate::l5_baseline::BaselineField;
use crate::l5_compare::{compare, Band};

pub const FIELDS: [&str; 4] = ["earth", "water", "fire", "wind"];

/// 单一主结论（一因一果）。
#[derive(Clone, Debug, PartialEq)]
pub struct Conclusion {
    pub title: String,
    pub description: String,
    /// 补充增量（成因）。
    pub cause: String,
}

/// 溯源（不妄语：可验证、可复现）。
#[derive(Clone, Debug, PartialEq)]
pub struct Traceability {
    pub baseline_id: String,
    pub object: String,
    pub at: String,
    pub reproducible: bool,
}

/// 完整诊断（schema 2：字段+模式+结论+溯源）。
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnosis {
    pub schema: u8,
    pub fields: [f64; 4],
    pub pattern: [Band; 4],
    pub conclusion: Conclusion,
    pub trace: Traceability,
}

fn band_name(b: Band) -> &'static str {
    b.name()
}

/// 合成描述：逐分量列出带（仅非平者优先排入）。
fn anomaly_phrase(pattern: &[Band; 4]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for i in 0..4 {
        if pattern[i] != Band::Ping {
            parts.push(format!("{}{}", band_name(pattern[i]), FIELDS[i]));
        }
    }
    if parts.is_empty() { "全场平（节律内）".to_string() } else { parts.join("+") }
}

/// 补充增量（成因）——按主导异常（亢优先于枯；同向取偏离更大者）。
/// 说明：成因只用对象自身场推导（不饮酒）；描述单一因果链（不邪淫）。
pub fn synthesize(fields: &[f64; 4], base: &BaselineField, pattern: &[Band; 4]) -> Conclusion {
    let anomaly = anomaly_phrase(pattern);
    // 偏离倍率（本底 0 防御）
    let dev = |i: usize| -> f64 {
        let bl = base.to_array()[i];
        if bl.abs() <= f64::EPSILON { if fields[i].abs() <= f64::EPSILON { 1.0 } else { f64::MAX } }
        else { (fields[i] / bl).abs() }
    };
    // 主导分量：亢优先（多个亢取偏离最大者）；无亢则取枯（偏离最小者）。
    // 一因一果（不邪淫）：只选一个主导分量写主标题；复合成因进入 description/cause。
    let mut lead: Option<(usize, Band)> = None;
    if (0..4).any(|i| pattern[i] == Band::Kang) {
        let mut best = 0.0f64;
        for i in 0..4 {
            if pattern[i] == Band::Kang {
                let d = dev(i);
                if lead.is_none() || d >= best { lead = Some((i, Band::Kang)); best = d; }
            }
        }
    } else {
        let mut min_d = f64::MAX;
        for i in 0..4 {
            if pattern[i] == Band::Ku {
                let d = dev(i);
                if d < min_d { min_d = d; lead = Some((i, Band::Ku)); }
            }
        }
    }

    let title;
    let cause;
    match lead {
        Some((i, Band::Kang)) => {
            title = format!("{}亢（{}越节律上限）", FIELDS[i].to_uppercase(), FIELDS[i]);
            cause = match FIELDS[i] {
                "fire" => "火行持续高速扰动超出水行涵养——火不济水".to_string(),
                "wind" => "风行频变过密，缺乏地与水的阻尼承接".to_string(),
                "earth" => "地行结构承压过载（分布/连接越限）".to_string(),
                _ => "水行涵养越限（熵流异常高）".to_string(),
            };
        }
        Some((i, Band::Ku)) => {
            title = format!("{}枯（{}越节律下限）", FIELDS[i].to_uppercase(), FIELDS[i]);
            cause = match FIELDS[i] {
                "water" => "水行涵养不足——缓冲/有序度低于下限".to_string(),
                "fire" => "火行活性不足——能量脉冲枯弱".to_string(),
                "earth" => "地行支撑不足——结构/分布收缩".to_string(),
                _ => "风行变动不足——僵滞缺乏流动".to_string(),
            };
        }
        _ => { // None（或不可能的 Ping）→ 平稳兜底
            title = "平稳运行（全场节律内）".to_string();
            cause = "无越界变量；存量（本底场）与当前场一致".to_string();
        }
    }
    let mut description = format!("对象场观察：{}。", anomaly);
    if pattern[1] == Band::Ku && pattern[2] == Band::Kang {
        description.push_str("水不济火：涵养不足而活性越限，成因为持续高速扰动超出缓冲能力。");
    }
    Conclusion { title, description, cause }
}

/// 主入口：compare + synthesize → Diagnosis（trace 由宿主注，默认可复现）。
pub fn diagnose(s: &[f64; 7], base: &BaselineField, object: &str) -> Diagnosis {
    let fields = crate::l5_senses::decompose(s);
    let pattern = compare(&fields, base);
    let conclusion = synthesize(&fields, base, &pattern);
    Diagnosis {
        schema: 2,
        fields,
        pattern,
        conclusion,
        trace: Traceability {
            baseline_id: format!("b-{}", base.established),
            object: object.to_string(),
            at: "local".to_string(),
            reproducible: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l5_baseline::BaselineField;

    fn base() -> BaselineField {
        BaselineField { earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0, object: "o", established: "learned" }
    }

    #[test]
    fn q1_calm_single_conclusion() {
        // 平稳：s 全 1（fields 全 1）→ pattern 全 Ping
        let s = [1.0; 7];
        let d = diagnose(&s, &base(), "obj");
        assert_eq!(d.pattern, [Band::Ping; 4]);
        assert!(d.conclusion.title.contains("平稳"), "{}", d.conclusion.title);
        assert!(d.conclusion.cause.contains("存量"), "{}", d.conclusion.cause);
        // 一因一果：title/description/cause 各一，无并列猜测
        assert!(!d.conclusion.description.contains("；或"), "不并列多因");
    }

    #[test]
    fn q2_fire_kang_single_cause() {
        // 火亢：t=2,a=2 → fire=(2+2)/2=2.0 > 1.618
        let s = [2.0, 1.0, 2.0, 1.0, 1.0, 1.0, 1.0];
        let d = diagnose(&s, &base(), "obj");
        assert_eq!(d.pattern[2], Band::Kang);
        assert!(d.conclusion.title.contains("fire"), "{}", d.conclusion.title);
        assert!(d.conclusion.title.contains("亢"), "{}", d.conclusion.title);
    }

    #[test]
    fn q3_water_ku_single_cause() {
        // 水枯：entropy=0.5
        let s = [1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0];
        let d = diagnose(&s, &base(), "obj");
        assert_eq!(d.pattern[1], Band::Ku);
        assert!(d.conclusion.title.contains("枯"), "{}", d.conclusion.title);
    }

    #[test]
    fn q4_fire_kang_plus_water_ku_single_title() {
        // 复合：fire 亢 + water 枯 → 亢优先（不邪淫：单一主标题 + 成因句注明水不济火）
        let s = [2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0];
        let d = diagnose(&s, &base(), "obj");
        assert_eq!(d.pattern[2], Band::Kang);
        assert_eq!(d.pattern[1], Band::Ku);
        assert!(d.conclusion.title.contains("fire"), "亢优先: {}", d.conclusion.title);
        assert!(d.conclusion.description.contains("水不济火"), "{}", d.conclusion.description);
        // 单一主结论：title 只有一个主名
        assert!(!d.conclusion.title.contains("；"), "title 不并列");
    }

    #[test]
    fn q6_pure_no_residue_repeatable() {
        let s = [2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0];
        let b = base();
        let a = diagnose(&s, &b, "obj");
        for _ in 0..3 {
            let c = diagnose(&s, &b, "obj");
            assert_eq!(c, a, "同输入同输出（纯、无缓存残留）");
        }
    }

    #[test]
    fn baseline_influences_pattern() {
        // 本底场 fire=0.5：当前 fire=1.0 → 偏离 2.0 → 亢（相对对象自身地图）
        let b = BaselineField { earth: 1.0, water: 1.0, fire: 0.5, wind: 1.0, object: "o", established: "learned" };
        let s = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]; // fire=(a+t)/2=1.0
        let d = diagnose(&s, &b, "obj");
        assert_eq!(d.pattern[2], Band::Kang, "相对自身地图的亢");
    }
}
