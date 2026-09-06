//! # 持久化（Persist）— 摩尼宝珠·记忆层
//!
//! 把内核"核心可恢复状态"序列化为紧凑 JSON 字符串（浏览器 localStorage /
//! native 文件共用同一格式），加载时优先恢复——**刷新页面后自我感不归零**。
//!
//! 设计取舍（零第三方依赖）：本模块只负责 **纯编解码**（无 I/O、无状态），
//! 字符串进出由宿主侧（wasm localStorage / native 文件）完成；
//! 编码输出标准 JSON（人类可读、可转发），解码器只解析**本模块生成的子集**
//! （键序无关、缺失即 None），因此无需引入完整 JSON 解析器。
//!
//! 范围（核心可恢复）：自我感 / 心海全景 / 能量储备 / 物态 / 时间戳。
//! 指令队列为瞬态不持久化；痕迹流逐条重放留待未来扩展。

/// 快照格式版本（不匹配即拒绝加载，从 0 锚点重启）。
pub const PERSIST_VERSION: u32 = 1;

/// 内核可恢复快照（核心子集）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KernelSnapshot {
    /// 格式版本。
    pub version: u32,
    /// 保存时刻（宿主 tick/时间戳）。
    pub timestamp: u64,
    /// 自我感强度 ∈[0,1]（刷新后不归零的验收核心）。
    pub self_intensity: f32,
    /// 心海全景：离 0 锚点距离 ∈[0,1]。
    pub anchor_distance: f32,
    /// 能量储备 ∈[0,1]。
    pub stored: f32,
    /// 物态码：0 能量态 / 1 气态 / 2 液态 / 3 固态。
    pub state_code: u32,
}

impl KernelSnapshot {
    /// 构造（字段统一钳制到合法域）。
    pub fn new(
        timestamp: u64,
        self_intensity: f32,
        anchor_distance: f32,
        stored: f32,
        state_code: u32,
    ) -> Self {
        Self {
            version: PERSIST_VERSION,
            timestamp,
            self_intensity: self_intensity.clamp(0.0, 1.0),
            anchor_distance: anchor_distance.clamp(0.0, 1.0),
            stored: stored.clamp(0.0, 1.0),
            state_code: state_code.min(3),
        }
    }
}

/// 序列化为紧凑 JSON（键序固定，便于人读与转发）。
pub fn encode(snap: &KernelSnapshot) -> String {
    format!(
        "{{\"v\":{},\"ts\":{},\"self\":{:.6},\"anchor\":{:.6},\"stored\":{:.6},\"state\":{}}}",
        snap.version, snap.timestamp, snap.self_intensity, snap.anchor_distance, snap.stored,
        snap.state_code
    )
}

fn field<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    for part in body.split(',') {
        if let Some((k, v)) = part.split_once(':') {
            if k.trim().trim_matches('"') == key {
                return Some(v.trim());
            }
        }
    }
    None
}

/// 反序列化（仅解析本模块生成的紧凑格式；缺失/非法/版本不符 → None）。
pub fn decode(s: &str) -> Option<KernelSnapshot> {
    let s = s.trim();
    if !(s.starts_with('{') && s.ends_with('}')) {
        return None;
    }
    let body = &s[1..s.len() - 1];
    let version: u32 = field(body, "v")?.parse().ok()?;
    if version != PERSIST_VERSION {
        return None;
    }
    let timestamp: u64 = field(body, "ts")?.parse().ok()?;
    let self_intensity: f32 = field(body, "self")?.parse().ok()?;
    let anchor: f32 = field(body, "anchor")?.parse().ok()?;
    let stored: f32 = field(body, "stored")?.parse().ok()?;
    let state: u32 = field(body, "state")?.parse().ok()?;
    // 域合法性守卫（非法数值拒绝加载 → 从 0 锚点启动）
    if !self_intensity.is_finite() || !anchor.is_finite() || !stored.is_finite() {
        return None;
    }
    Some(KernelSnapshot::new(timestamp, self_intensity, anchor, stored, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_all_fields() {
        let s = KernelSnapshot::new(4096, 0.423, 0.187, 0.661, 2);
        let text = encode(&s);
        let back = decode(&text).expect("自编码必须可解码");
        assert_eq!(back, s);
        assert!(text.starts_with('{') && text.ends_with('}'), "输出为 JSON 对象");
        // 人类可读性：JSON 键都在
        for key in ["\"v\"", "\"self\"", "\"anchor\"", "\"stored\"", "\"state\""] {
            assert!(text.contains(key), "缺 {key}: {text}");
        }
    }

    #[test]
    fn fields_are_clamped_on_construct() {
        let s = KernelSnapshot::new(1, 1.7, -0.2, 2.0, 99);
        assert_eq!(s.self_intensity, 1.0);
        assert_eq!(s.anchor_distance, 0.0);
        assert_eq!(s.stored, 1.0);
        assert_eq!(s.state_code, 3);
    }

    #[test]
    fn garbage_or_wrong_version_returns_none() {
        assert!(decode("").is_none());
        assert!(decode("hello").is_none());
        assert!(decode("{\"v\":9,\"ts\":1,\"self\":0.1,\"anchor\":0.1,\"stored\":0.1,\"state\":1}").is_none());
        assert!(decode("{\"ts\":1}").is_none(), "缺字段拒绝");
        assert!(decode("{\"v\":1,\"ts\":x,\"self\":0.1,\"anchor\":0.1,\"stored\":0.1,\"state\":1}").is_none());
        assert!(decode("{\"v\":1,\"ts\":1,\"self\":NaN,\"anchor\":0.1,\"stored\":0.1,\"state\":1}").is_none());
    }

    #[test]
    fn key_order_independent() {
        // 键序打乱仍可解析（子集解析器按键查找）
        let text = "{\"state\":3,\"stored\":0.2,\"anchor\":0.9,\"self\":0.7,\"ts\":77,\"v\":1}";
        let s = decode(text).expect("键序无关");
        assert_eq!(s.state_code, 3);
        assert!((s.self_intensity - 0.7).abs() < 1e-4);
    }
}
