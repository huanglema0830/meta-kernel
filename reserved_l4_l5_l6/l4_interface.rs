// =====================================================================
// 【L4·框架·拒绝层】预留接口（NOT COMPILED — 见本目录 README.md）
// 定稿依据：docs/L4_APPLICATION_FRAMEWORK_DESIGN v1.0（发起人审核通过）
// 戒律：拒绝层（L4）——拒绝无溯源表达、拒绝越权造内核事实、拒绝低阈值抖动。
// 以下仅为契约骨架签名，供实现轮（npb-appkit crate）激活；本文件不编译。
// =====================================================================

/// 应用最小接入契约（AppSpec）。
/// 应用只需声明订阅意图与呈现方式，框架承担连接/状态机/翻译。
pub struct AppSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// 订阅意图：lifecycle | resonance | low_energy | ...
    pub subscribes: &'static [&'static str],
    /// 本地化：默认 "zh"
    pub locale: &'static str,
    /// 意图原语：suggest_next | journal（一期基础原语）
    pub intents: &'static [&'static str],
}

/// 归一内核事件（由 EventPipe 从 L3 SSE/指令产出；方向见 L4 §3.3）。
pub enum KernelEvent {
    /// 点亮/正向推进（StateChanged→低 code、Compound、Resonance、Self 升）
    Awaken,
    /// 保持（粒子成形、无变化）
    Hold,
    /// 回落（StateChanged→高 code、LowEnergy、Habit 重复）
    Settle,
    /// 回归（entropy→1 或显式 reset）
    Reset,
}

/// 显化生命周期（0 ∪ {10..=99}，圈层×微步；纯函数状态机）。
/// 转移规则：同带 ±1；跨带需连续 ≥2 同向事件（防抖）；99 静默窗 N 后回融 0。
pub trait LifecycleEngine {
    /// 纯函数：给定当前状态与事件，返回新状态（确定性，可单测）。
    fn step(state: u16, ev: KernelEvent) -> u16;
    /// 圈层名（命名 registry 提供，如 12 → "萌发·第2步"）。
    fn band_name(state: u16) -> &'static str;
}

/// 命名 registry（双轨：内部符号 ↔ 外部名）。
/// v1.0 补充：条目统一预留 `twin` 字段位（二期孪生指纹挂载，一期不占用）。
pub struct NamedEntity {
    pub internal: &'static str,
    pub external: &'static str,
    /// 预留：二期孪生指纹（twin_fingerprint=!fp 配对语义）；一期恒空。
    pub twin: Option<u64>,
}

/// 语言组织·陈述层：把指令/事件字段翻成一句可溯源陈述（不增义）。
/// 输出必须可回指原事件字段（source 必填）。
pub struct Statement {
    pub text: String,
    pub source: &'static str, // e.g. "instruction#compound|state_change"
}

/// Speaker：三层流水线（指称→陈述→意图）的框架侧实现契约。
pub trait Speaker {
    fn statement_of(ev: &KernelEvent, entity: &NamedEntity) -> Statement;
    /// 意图原语（一期：suggest_next / journal 文案模板）。
    fn intent_of(kind: &str, st: &Statement) -> String;
}
