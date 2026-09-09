//! L5 · 地水火风拆解（l5_senses）。
//!
//! 把对象场七维采样 `S=(t,f,a,φ,x,H,τ)` 拆解为四个本征分量（号脉语境）。
//! 观测投影（设计 v2.0 §2.1 示意；定死为确定性纯函数，本底场标定时可校）：
//! - 地 earth = (x + τ)/2   （空间形态与结构拓扑 → 承载/稳定）
//! - 水 water = H            （熵 → 涵养/有序缓冲）
//! - 火 fire  = (a + t)/2    （幅度能量与节奏 → 活性/脉冲）
//! - 风 wind  = (f + φ)/2    （频率与相位 → 变动/扩散）

/// 分量顺序（与 [FIELDS] 一致）。
pub const FIELDS: [&str; 4] = ["earth", "water", "fire", "wind"];

/// 七维采样 → 四场分量（纯函数；输入视为对对象自身场的观测）。
pub fn decompose(s: &[f64; 7]) -> [f64; 4] {
    // s 下标：0=t,1=f,2=a,3=phi,4=x,5=H(entropy),6=tau
    [
        (s[4] + s[6]) / 2.0, // 地：x + tau
        s[5],               // 水：entropy
        (s[2] + s[0]) / 2.0, // 火：a + t
        (s[1] + s[3]) / 2.0, // 风：f + phi
    ]
}


/// 场域采样（L5 设计 §5.1：cloud-probe 输出格式与此一致）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldReading {
    /// 七维场域状态向量 S=(t,f,a,φ,x,H,τ)。
    pub s: [f64; 7],
    /// 采样标识/时间（可空，宿主注）。
    pub at: Option<&'static str>,
}

impl FieldReading {
    /// 由七维数组构造（默认无 at）。
    pub fn new(s: [f64; 7]) -> Self {
        Self { s, at: None }
    }
    pub fn with_at(s: [f64; 7], at: &'static str) -> Self {
        Self { s, at: Some(at) }
    }
    /// 手写 JSON（零依赖；cloud-probe 输出载体）。
    pub fn to_json(&self) -> String {
        let v = self.s.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        format!("{{\"schema\":1,\"s\":[{v}],\"at\":\"{}\"}}", self.at.unwrap_or(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_matches_projection() {
        // s=(t=1,f=2,a=3,phi=4,x=5,H=6,tau=7)
        let s = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let f = decompose(&s);
        assert!((f[0] - 6.0).abs() < 1e-9, "earth=(x+tau)/2=(5+7)/2=6, got {}", f[0]);
        assert!((f[1] - 6.0).abs() < 1e-9, "water=H=6");
        assert!((f[2] - 2.0).abs() < 1e-9, "fire=(a+t)/2=(3+1)/2=2, got {}", f[2]);
        assert!((f[3] - 3.0).abs() < 1e-9, "wind=(f+phi)/2=(2+4)/2=3");
    }

    #[test]
    fn field_reading_json_matches_contract() {
        let r = FieldReading::with_at([1.0, 0.5, 2.0, 1.0, 1.0, 1.0, 1.0], "20260909");
        let j = r.to_json();
        assert!(j.starts_with("{\"schema\":1,\"s\":["), "{j}");
        assert!(j.contains("\"at\":\"20260909\""), "{j}");
        let r2 = FieldReading::new([1.0; 7]);
        assert_eq!(r2.s.len(), 7);
        assert_eq!(r2.at, None);
    }

    #[test]
    fn decompose_deterministic_pure() {
        let s = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5];
        let a = decompose(&s);
        let b = decompose(&s);
        assert_eq!(a, b, "同输入同输出（纯）");
    }
}
