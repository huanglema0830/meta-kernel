//! L5 · 语境模块（l5_context）—— 诊断层新增：**类型 / 时间 / 历史 / 环境**。
//!
//! 设计见 `docs/L5_DESIGN.md`（v2.1 §1/§2）与 `docs/GENE_LIBRARY_DESIGN.md`（§2.2 / §5.2）。
//!
//! 作用：**语境决定调用哪条场景公式**。同一对象在不同场景下应得到不同（但可复算）的本底场——
//! 语境就是"在什么条件下做这次诊断"的显式声明。
//!
//! - **语境采集**：[`Context::capture`] 从场域七维采样（+ 可选外部标注标签）提取四要素；
//! - **场景识别**：[`Context::scene_id`] 由四要素确定性派生场景标识（纯函数）；
//! - **场景公式**：场景标识 → 基因库场景公式层（[`crate::gene_library::SceneGene`]）的本底场。
//!
//! 纯逻辑（零依赖）：四要素的分桶阈值为**启发式投影**（与 `l5_senses::decompose` 同性质），
//! 可在本底场标定时校准；此处固定为确定性纯函数，保证同输入同输出。

use crate::gene_library::fnv1a64;

/// 类型（对象类别）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Hardware,
    Software,
    Plant,
    Animal,
    Geology,
    Unknown,
}

/// 时间（时段/节奏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeBucket {
    Idle,
    Active,
    Burst,
    Unknown,
}

/// 历史（有无异常史 / 熵积累）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum History {
    Clean,
    Watch,
    Anomalous,
    Unknown,
}

/// 环境（外部激励/温度条件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Cool,
    Normal,
    Hot,
    Unknown,
}

impl Kind {
    pub fn code(&self) -> u8 {
        match self {
            Kind::Hardware => 1,
            Kind::Software => 2,
            Kind::Plant => 3,
            Kind::Animal => 4,
            Kind::Geology => 5,
            Kind::Unknown => 0,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Hardware => "hardware",
            Kind::Software => "software",
            Kind::Plant => "plant",
            Kind::Animal => "animal",
            Kind::Geology => "geology",
            Kind::Unknown => "unknown",
        }
    }
}
impl TimeBucket {
    pub fn code(&self) -> u8 {
        match self {
            TimeBucket::Idle => 1,
            TimeBucket::Active => 2,
            TimeBucket::Burst => 3,
            TimeBucket::Unknown => 0,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            TimeBucket::Idle => "idle",
            TimeBucket::Active => "active",
            TimeBucket::Burst => "burst",
            TimeBucket::Unknown => "unknown",
        }
    }
}
impl History {
    pub fn code(&self) -> u8 {
        match self {
            History::Clean => 1,
            History::Watch => 2,
            History::Anomalous => 3,
            History::Unknown => 0,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            History::Clean => "clean",
            History::Watch => "watch",
            History::Anomalous => "anomalous",
            History::Unknown => "unknown",
        }
    }
}
impl Environment {
    pub fn code(&self) -> u8 {
        match self {
            Environment::Cool => 1,
            Environment::Normal => 2,
            Environment::Hot => 3,
            Environment::Unknown => 0,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Environment::Cool => "cool",
            Environment::Normal => "normal",
            Environment::Hot => "hot",
            Environment::Unknown => "unknown",
        }
    }
}

/// 语境（四要素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub kind: Kind,
    pub time: TimeBucket,
    pub history: History,
    pub environment: Environment,
}

/// 由外部标注推断类型（不区分大小写；含中英文关键词）。
fn kind_of_tag(tag: &str) -> Kind {
    let t = tag.to_ascii_lowercase();
    let has = |k: &str| t.contains(k) || tag.contains(k);
    if has("notebook") || has("laptop") || has("hardware") || has("pc") || tag.contains("笔记本") || tag.contains("硬件") {
        Kind::Hardware
    } else if has("software") || has("app") || tag.contains("软件") {
        Kind::Software
    } else if has("plant") || tag.contains("植物") {
        Kind::Plant
    } else if has("animal") || tag.contains("动物") {
        Kind::Animal
    } else if has("geology") || has("rock") || tag.contains("地质") {
        Kind::Geology
    } else {
        Kind::Unknown
    }
}

impl Context {
    /// 构造（显式给四要素）。
    pub fn new(kind: Kind, time: TimeBucket, history: History, environment: Environment) -> Self {
        Self { kind, time, history, environment }
    }

    /// **语境采集**：从场域七维采样 `S=(t,f,a,φ,x,H,τ)` + 可选外部标注，提取四要素。
    ///
    /// 分桶（启发式投影，确定性）：
    /// - 时间 ← `t`（s[0] 节奏/时刻）：`<0.34 → Idle`，`<0.67 → Active`，否则 `Burst`；
    /// - 历史 ← `H`（s[5] 熵/历时积累）：`<0.5 → Clean`，`<1.0 → Watch`，否则 `Anomalous`；
    /// - 环境 ← `a`（s[2] 幅度/外部激励）：`<0.5 → Cool`，`<1.0 → Normal`，否则 `Hot`；
    /// - 类型 ← 外部标注标签（探针/设备台注入）；未标注 → `Unknown`。
    ///
    /// 非有限值（NaN/∞）一律归 `Unknown`（不猜测）。
    pub fn capture(s: &[f64; 7], tag: Option<&str>) -> Self {
        let bucket3 = |v: f64, lo: f64, hi: f64| -> u8 {
            if !v.is_finite() {
                0
            } else if v < lo {
                1
            } else if v < hi {
                2
            } else {
                3
            }
        };
        let time = match bucket3(s[0], 0.34, 0.67) {
            1 => TimeBucket::Idle,
            2 => TimeBucket::Active,
            3 => TimeBucket::Burst,
            _ => TimeBucket::Unknown,
        };
        let history = match bucket3(s[5], 0.5, 1.0) {
            1 => History::Clean,
            2 => History::Watch,
            3 => History::Anomalous,
            _ => History::Unknown,
        };
        let environment = match bucket3(s[2], 0.5, 1.0) {
            1 => Environment::Cool,
            2 => Environment::Normal,
            3 => Environment::Hot,
            _ => Environment::Unknown,
        };
        let kind = match tag {
            Some(t) if !t.is_empty() => kind_of_tag(t),
            _ => Kind::Unknown,
        };
        Self { kind, time, history, environment }
    }

    /// **场景识别**：由四要素确定性派生场景标识（纯函数，非零）。
    ///
    /// 标识 = fnv1a(四要素编码) 截断为 u32，并保证不为 0（0 保留给"无场景"）。
    pub fn scene_id(&self) -> u32 {
        let bytes = [
            self.kind.code(),
            self.time.code(),
            self.history.code(),
            self.environment.code(),
        ];
        let h = fnv1a64(fnv1a64(0xcbf2_9ce4_8422_2325, b"l5-scene"), &bytes);
        let id = (h ^ (h >> 32)) as u32;
        if id == 0 { 1 } else { id }
    }

    /// 场景参数编码（四要素 → `[f64; 4]`，供场景公式层参数化）。
    pub fn params(&self) -> [f64; 4] {
        [
            self.kind.code() as f64,
            self.time.code() as f64,
            self.history.code() as f64,
            self.environment.code() as f64,
        ]
    }

    /// 可读场景标签（如 `"hardware/active/clean/normal"`）。
    pub fn label(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.kind.label(),
            self.time.label(),
            self.history.label(),
            self.environment.label()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_buckets_by_thresholds() {
        // t=0.1 → Idle；H=0.2 → Clean；a=0.3 → Cool
        let c = Context::capture(&[0.1, 0.0, 0.3, 0.0, 0.0, 0.2, 0.0], None);
        assert_eq!(c.time, TimeBucket::Idle);
        assert_eq!(c.history, History::Clean);
        assert_eq!(c.environment, Environment::Cool);
        assert_eq!(c.kind, Kind::Unknown, "无标注 → Unknown（不猜测）");
        // 中值
        let c2 = Context::capture(&[0.5, 0.0, 0.7, 0.0, 0.0, 0.7, 0.0], None);
        assert_eq!(c2.time, TimeBucket::Active);
        assert_eq!(c2.history, History::Watch);
        assert_eq!(c2.environment, Environment::Normal);
        // 高值
        let c3 = Context::capture(&[0.9, 0.0, 1.5, 0.0, 0.0, 2.0, 0.0], None);
        assert_eq!(c3.time, TimeBucket::Burst);
        assert_eq!(c3.history, History::Anomalous);
        assert_eq!(c3.environment, Environment::Hot);
    }

    #[test]
    fn capture_kind_from_tag() {
        let s = [0.5; 7];
        assert_eq!(Context::capture(&s, Some("2015-notebook")).kind, Kind::Hardware);
        assert_eq!(Context::capture(&s, Some("笔记本")).kind, Kind::Hardware);
        assert_eq!(Context::capture(&s, Some("plant-pot")).kind, Kind::Plant);
        assert_eq!(Context::capture(&s, Some("地质样本")).kind, Kind::Geology);
        assert_eq!(Context::capture(&s, Some("")).kind, Kind::Unknown);
        assert_eq!(Context::capture(&s, None).kind, Kind::Unknown);
    }

    #[test]
    fn capture_handles_non_finite() {
        let s = [f64::NAN, 0.0, f64::INFINITY, 0.0, 0.0, f64::NAN, 0.0];
        let c = Context::capture(&s, None);
        assert_eq!(c.time, TimeBucket::Unknown);
        assert_eq!(c.history, History::Unknown);
        assert_eq!(c.environment, Environment::Unknown);
    }

    #[test]
    fn scene_id_deterministic_and_distinct() {
        let a = Context::new(Kind::Hardware, TimeBucket::Idle, History::Clean, Environment::Normal);
        let b = a;
        assert_eq!(a.scene_id(), b.scene_id(), "同语境同标识（纯函数）");
        assert_ne!(a.scene_id(), 0, "标识非零");
        let c = Context::new(Kind::Hardware, TimeBucket::Burst, History::Clean, Environment::Normal);
        assert_ne!(a.scene_id(), c.scene_id(), "不同语境不同标识");
        let d = Context::new(Kind::Software, TimeBucket::Idle, History::Clean, Environment::Normal);
        assert_ne!(a.scene_id(), d.scene_id());
    }

    #[test]
    fn params_and_label() {
        let c = Context::new(Kind::Hardware, TimeBucket::Active, History::Clean, Environment::Normal);
        assert_eq!(c.params(), [1.0, 2.0, 1.0, 2.0]);
        assert_eq!(c.label(), "hardware/active/clean/normal");
    }

    #[test]
    fn unknown_context_is_stable() {
        let u = Context::capture(&[f64::NAN; 7], None);
        assert_eq!(u.scene_id(), Context::capture(&[f64::NAN; 7], None).scene_id());
        assert!(u.scene_id() != 0);
        assert_eq!(u.label(), "unknown/unknown/unknown/unknown");
    }
}
