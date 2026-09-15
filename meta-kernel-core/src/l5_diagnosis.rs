//! L5 · 诊断结论生成（l5_diagnosis）——三量运算。
//!
//! 存量 = 本底场（对象自身地图）；变量 = 当前场偏移（亢/枯/平）；
//! 补充增量 = 成因（是什么导致偏移）。
//! 戒律落实：不邪淫（一次号脉单一主结论）；不妄语（trace 可复现）；
//! 不饮酒（只用自身场数据推导）。纯函数、无状态。
//!
//! ## v0.123 · 诊断证据（世界模型匹配度 + 预测误差）
//! 发起人要求：**诊断不能只看当前场域，还要看世界模型中的历史模式**，且**预测误差**
//! 要成为确信度的依据之一（须随结论携带）。
//! 落点（见 [`crate::l5_evidence`]）：
//! - 结论**本身**仍由场域自身推导（不饮酒 → 因果链不被外部量改写）；
//! - 证据只修正 **`confidence`**（确信度），并把修正过程写进 `Conclusion::basis`；
//! - **无证据时逐位等价于 v0.122**（`basis = None`、确信度不变）。

use crate::l5_baseline::BaselineField;
use crate::l5_compare::{compare, Band};
use crate::l5_evidence::{self, Adjustment, Evidence, Gains};

pub const FIELDS: [&str; 4] = ["earth", "water", "fire", "wind"];

/// 单一主结论（一因一果）。
#[derive(Clone, Debug, PartialEq)]
pub struct Conclusion {
    pub title: String,
    pub description: String,
    /// 补充增量（成因）。
    pub cause: String,
    /// 诊断确信度（0..1）：由异常维度数与偏离幅度映射——L6 据此自动选语气等级（菩萨戒）。
    pub confidence: f64,
    /// 建议键（语言无关，如 advice.earth.kang）——L6 按用户语言渲染。
    pub suggestion_key: String,
    /// 建议默认文本（中文，兼容 CLI/日志）。
    pub suggestion: String,
    /// **确信度依据**（v0.123）：世界模型匹配度 / 预测误差各自的修正量与说明。
    /// `None` = 无外部证据（确信度仅由场域自身推出，与 v0.122 行为一致）。
    pub basis: Option<Adjustment>,
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
pub fn synthesize_with_evidence(
    fields: &[f64; 4],
    base: &BaselineField,
    pattern: &[Band; 4],
    ev: &Evidence,
    gains: &Gains,
) -> Conclusion {
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
    // 确信度（L6 语气映射输入）：异常维度数与偏离幅度越高 → 越确信
    let anomaly_n = pattern.iter().filter(|b| **b != Band::Ping).count();
    let max_dev = (0..4).map(dev).fold(0.0f64, f64::max);
    let base_conf = base_confidence(anomaly_n, max_dev);
    // **诊断证据修正**（v0.123）：世界模型匹配度高 / 预测误差低 → 更确信，反之更不确信。
    // 无证据 → 修正量 0 且 `basis = None` → 与 v0.122 逐位一致。
    let adj = l5_evidence::adjust(base_conf, ev, gains);
    let (confidence, basis) = if ev.is_empty() {
        (base_conf, None)
    } else {
        (adj.final_confidence, Some(adj))
    };
    // 建议（单一、语言无关键 + 默认中文文本）
    let (suggestion_key, suggestion) = advice(&lead);
    Conclusion { title, description, cause, confidence, suggestion_key, suggestion, basis }
}

/// **基础确信度**：仅由场域自身推导（异常维度数 + 最大偏离幅度）——证据修正的起点。
fn base_confidence(anomaly_n: usize, max_dev: f64) -> f64 {
    (0.55_f64
        + if anomaly_n >= 1 { 0.2 } else { 0.0 }
        + if max_dev >= crate::l4::threshold::GOLDEN_HIGH { 0.25 } else { 0.0 })
    .min(1.0)
}

/// 兼容入口：**不带证据**（等价于 v0.122 行为；确信度仅由场域自身推出）。
pub fn synthesize(fields: &[f64; 4], base: &BaselineField, pattern: &[Band; 4]) -> Conclusion {
    synthesize_with_evidence(fields, base, pattern, &Evidence::NONE, &Gains::default())
}

/// 建议映射：主导分量 × 带 → （语言无关键, 默认中文文本）。单一建议（不邪淫）。
fn advice(lead: &Option<(usize, Band)>) -> (String, String) {
    let (i, band) = match lead {
        Some(v) => *v,
        None => return ("advice.calm".to_string(), "维持现状：场域在节律内，无需干预。".to_string()),
    };
    let key = match (FIELDS[i], band) {
        ("earth", Band::Kang) => ("advice.earth.kang", "减少常驻后台与连接数（关闭不必要程序/服务）。"),
        ("water", Band::Kang) => ("advice.water.kang", "清理进程与自启项，降低并发。"),
        ("fire", Band::Kang) => ("advice.fire.kang", "暂停高耗任务，给系统喘息空间。"),
        ("wind", Band::Kang) => ("advice.wind.kang", "降低波动源（关闭频繁读写/网络抖动源）。"),
        ("earth", Band::Ku) => ("advice.earth.ku", "补充资源（释放磁盘/内存，检查外设连接）。"),
        ("water", Band::Ku) => ("advice.water.ku", "补充缓冲（释放内存、清理缓存）。"),
        ("fire", Band::Ku) => ("advice.fire.ku", "补充能量（接通电源、检查供电/负载）。"),
        ("wind", Band::Ku) => ("advice.wind.ku", "恢复流动（检查网络与端口连通）。"),
        _ => ("advice.calm", "维持现状：场域在节律内，无需干预。"),
    };
    (key.0.to_string(), key.1.to_string())
}

/// 主入口：compare + synthesize → Diagnosis（trace 由宿主注，默认可复现）。
pub fn diagnose(s: &[f64; 7], base: &BaselineField, object: &str) -> Diagnosis {
    diagnose_with_evidence(s, base, object, &Evidence::NONE, &Gains::default())
}

/// 主入口（带证据）：世界模型匹配度 + 预测误差 参与确信度，并随结论携带依据。
pub fn diagnose_with_evidence(
    s: &[f64; 7],
    base: &BaselineField,
    object: &str,
    ev: &Evidence,
    gains: &Gains,
) -> Diagnosis {
    let fields = crate::l5_senses::decompose(s);
    let pattern = compare(&fields, base);
    let conclusion = synthesize_with_evidence(&fields, base, &pattern, ev, gains);
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

    fn ev(w: Option<f64>, pe: Option<f64>) -> Evidence {
        Evidence { world_match: w, prediction_error: pe }
    }

    /// **未饱和样本**（单一水枯）：基础确信度 **0.75** —— 证据修正可被清晰观测。
    /// 为何不用复合样本：火亢偏离已达 2.0 ≥ 1.618 → 基础确信度**顶到 1.0**，
    /// 正向证据会被上界吃掉，用它测"提高"会得到**假阴性**。
    fn s_soft() -> [f64; 7] {
        [1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0]
    }

    /// **饱和样本**（火亢 + 水枯）：基础确信度 1.0，用于验证上界与"负向证据仍生效"。
    fn s_mix() -> [f64; 7] {
        [2.0, 1.0, 2.0, 1.0, 1.0, 0.5, 1.0]
    }

    #[test]
    fn no_evidence_is_bitwise_same_as_before() {
        for s in [s_soft(), s_mix()] {
            let a = diagnose(&s, &base(), "obj");
            let b = diagnose_with_evidence(&s, &base(), "obj", &Evidence::NONE, &Gains::default());
            assert_eq!(a, b, "无证据必须与 v0.122 行为逐位一致");
            assert!(a.conclusion.basis.is_none(), "无证据不产生依据");
        }
    }

    #[test]
    fn world_match_high_raises_confidence_low_lowers() {
        let g = Gains::default();
        let hit = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(Some(1.0), None), &g);
        let mid = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(Some(0.5), None), &g);
        let miss = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(Some(0.0), None), &g);
        assert!(
            hit.conclusion.confidence > mid.conclusion.confidence,
            "匹配高 → 确信度高：{} vs {}",
            hit.conclusion.confidence,
            mid.conclusion.confidence
        );
        assert!(mid.conclusion.confidence > miss.conclusion.confidence, "匹配低 → 确信度低");
        // 证据只动确信度，**不改结论**（一因一果 / 不饮酒）
        assert_eq!(hit.conclusion.title, miss.conclusion.title);
        assert_eq!(hit.conclusion.cause, miss.conclusion.cause);
        assert_eq!(hit.conclusion.suggestion_key, miss.conclusion.suggestion_key);
        assert_eq!(hit.pattern, miss.pattern);
        let b = hit.conclusion.basis.as_ref().expect("依据必须随结论携带");
        assert_eq!(b.world_match, Some(1.0));
        assert!(b.world_adjust > 0.0);
        assert!(b.note.contains("世界模型匹配度"), "{}", b.note);
    }

    #[test]
    fn prediction_error_low_raises_confidence_high_lowers() {
        let g = Gains::default();
        let low = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(None, Some(0.002)), &g);
        let high = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(None, Some(0.40)), &g);
        assert!(
            low.conclusion.confidence > high.conclusion.confidence,
            "误差低 → 确信度高：{} vs {}",
            low.conclusion.confidence,
            high.conclusion.confidence
        );
        // 发起人要求：**结论携带预测误差**作为确信度依据
        let b = low.conclusion.basis.as_ref().expect("依据必须随结论携带");
        assert_eq!(b.prediction_error, Some(0.002), "必须携带误差原值");
        assert!(b.error_score.expect("须有归一化分") > 0.9);
        assert!(b.pe_adjust > 0.0);
        assert!(b.note.contains("预测误差 0.0020"), "{}", b.note);
        let bh = high.conclusion.basis.as_ref().unwrap();
        assert!(bh.pe_adjust < 0.0, "高误差 → 负修正");
        assert!(bh.error_score.unwrap() < 0.2);
    }

    #[test]
    fn both_evidences_compose_and_stay_bounded() {
        let g = Gains::default();
        let both = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(Some(0.8), Some(0.01)), &g);
        let world_only = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(Some(0.8), None), &g);
        let base_only = diagnose_with_evidence(&s_soft(), &base(), "obj", &Evidence::NONE, &g);
        let bad = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(Some(0.2), Some(0.40)), &g);
        assert!(both.conclusion.confidence > world_only.conclusion.confidence, "两项同向应叠加");
        assert!(world_only.conclusion.confidence > base_only.conclusion.confidence);
        assert!(base_only.conclusion.confidence > bad.conclusion.confidence);
        for d in [&both, &world_only, &base_only, &bad] {
            let c = d.conclusion.confidence;
            assert!((0.0..=1.0).contains(&c), "确信度必须界定在 0..1：{c}");
        }
    }

    #[test]
    fn negative_evidence_still_works_when_saturated() {
        // 基础确信度已顶到 1.0 时：正向证据被上界吃掉，但**负向证据仍必须生效**——
        // 否则"与世界模型不匹配就该更不确信"会被上界静默吞掉（那才是真正的缺陷）。
        let g = Gains::default();
        let nom = diagnose_with_evidence(&s_mix(), &base(), "obj", &Evidence::NONE, &g);
        assert!((nom.conclusion.confidence - 1.0).abs() < 1e-12, "确认该样本确实饱和");
        let bad = diagnose_with_evidence(&s_mix(), &base(), "obj", &ev(Some(0.0), Some(0.5)), &g);
        assert!(bad.conclusion.confidence < nom.conclusion.confidence, "饱和下负向证据必须仍生效");
        assert!(bad.conclusion.confidence < 1.0);
    }

    #[test]
    fn immature_world_evidence_barely_moves_confidence() {
        use crate::l1_field_parse::FieldReading;
        use crate::l3_world::WorldModel;
        let g = Gains::default();
        let fields = crate::l5_senses::decompose(&s_soft());
        let f = FieldReading {
            earth: fields[0],
            water: fields[1],
            fire: fields[2],
            wind: fields[3],
            confidence: 1.0,
        };
        // 同一场域形状：观测 1 次（不成熟）vs 观测 10 次（成熟）
        let mut young = WorldModel::new();
        young.observe_page("h", "h/a", &f, 100);
        let mut mature = WorldModel::new();
        for _ in 0..10 {
            mature.observe_page("h", "h/a", &f, 100);
        }
        let my = l5_evidence::match_to_world(&f, &young, Some("h/a"), &g);
        let mm = l5_evidence::match_to_world(&f, &mature, Some("h/a"), &g);
        assert!((my.maturity - 0.125).abs() < 1e-12, "1 次观测 → 成熟度 1/8");
        assert!(mm.score > 0.99, "成熟世界里的同形场 → 强匹配 {}", mm.score);
        assert!(
            (my.score - 0.5).abs() < (mm.score - 0.5).abs(),
            "不成熟 → 更靠中性（不妄语：没把握就不动结论）"
        );
        let dy = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(Some(my.score), None), &g);
        let dm = diagnose_with_evidence(&s_soft(), &base(), "obj", &ev(Some(mm.score), None), &g);
        assert!(dm.conclusion.confidence > dy.conclusion.confidence, "成熟世界的强匹配修正更大");
        assert!(dy.conclusion.confidence > 0.75, "修正须仍为正（且已被成熟度削弱）");
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
