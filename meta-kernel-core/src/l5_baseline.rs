//! L5 · 本底场建立与存储（l5_baseline）。
//!
//! 本底场 = 对象自身的内部标准地图（健康态地水火风），由对象场学习建立。
//! - 建立：`learn`（多次采样分量 → 各分量均值）；
//! - 存储：纯编解码（`to_json`/`from_json`，零依赖）——**持久 IO 由宿主负责**
//!   （native 文件系统 / 网关 / L6 侧），内核保持 wasm 可编（无 std::fs）。
//! 戒律边界（不偷盗）：只存对象自身地图 + 标识；不存诊断过程数据。

/// 本底场（内部标准地图）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaselineField {
    /// 地（承载/结构）。
    pub earth: f64,
    /// 水（涵养/有序）。
    pub water: f64,
    /// 火（活性/能量）。
    pub fire: f64,
    /// 风（变动/频率）。
    pub wind: f64,
    /// 对象标识（宿主注入；如 "2015-notebook"）。
    pub object: &'static str,
    /// 标定时间/标识（宿主注入或 learn 时记 "learned"）。
    pub established: &'static str,
}

impl BaselineField {
    /// 由学习期分量采样建立：各分量取均值（对象/时间由宿主注，默认占位）。
    pub fn learn(samples: &[[f64; 4]], object: &'static str) -> Self {
        let n = samples.len().max(1) as f64;
        let mut acc = [0.0f64; 4];
        for s in samples {
            for i in 0..4 {
                acc[i] += s[i];
            }
        }
        Self {
            earth: acc[0] / n,
            water: acc[1] / n,
            fire: acc[2] / n,
            wind: acc[3] / n,
            object,
            established: "learned",
        }
    }

    /// L5 补足：自适应基线（无预设基线时）——从试探采样提取正常波动范围：
    /// 每场取中位数为基线，返回平均相对波动 spread（供 confidence 参考）。
    pub fn adaptive_baseline(samples: &[[f64; 4]]) -> (Self, f64) {
        let mut out = [1.0f64; 4];
        let mut spread_acc = 0.0f64;
        for i in 0..4 {
            let mut v: Vec<f64> = samples.iter().map(|s| s[i]).collect();
            if v.is_empty() {
                continue;
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let med = v[v.len() / 2];
            let lo = v[0];
            let hi = v[v.len() - 1];
            out[i] = if med.abs() < 1e-9 { 1.0 } else { med };
            spread_acc += if med.abs() < 1e-9 { 0.0 } else { (hi - lo) / med.abs() };
        }
        (
            Self { earth: out[0], water: out[1], fire: out[2], wind: out[3],
                   object: "adaptive", established: "derived" },
            spread_acc / 4.0,
        )
    }

    /// 分量数组（顺序同 FIELDS）。
    pub fn to_array(&self) -> [f64; 4] {
        [self.earth, self.water, self.fire, self.wind]
    }

    /// 手写 JSON 编码（零依赖；供宿主持久）。
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\":2,\"object\":\"{}\",\"established\":\"{}\",\"fields\":{{\"earth\":{},\"water\":{},\"fire\":{},\"wind\":{}}}}}",
            self.object, self.established, self.earth, self.water, self.fire, self.wind
        )
    }

    /// 手写 JSON 解码（宽松：从既有 to_json 形态还原）。
    pub fn from_json(j: &str) -> Option<Self> {
        let f = |k: &str| -> Option<f64> {
            let n = format!("\"{k}\"");
            let i = j.find(&n)?;
            let r = &j[i + n.len()..];
            let c = r.find(':')? + 1;
            let v: String = r[c..].chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == 'e')
                .collect();
            v.parse().ok()
        };
        Some(Self {
            earth: f("earth")?,
            water: f("water")?,
            fire: f("fire")?,
            wind: f("wind")?,
            object: "obj",
            established: "loaded",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l5_senses::FIELDS;

    #[test]
    fn learn_averages_samples() {
        let samples = [
            [1.0, 2.0, 3.0, 4.0],
            [3.0, 4.0, 5.0, 6.0],
            [5.0, 6.0, 7.0, 8.0],
        ];
        let b = BaselineField::learn(&samples, "test-obj");
        assert!((b.earth - 3.0).abs() < 1e-9);
        assert!((b.water - 4.0).abs() < 1e-9);
        assert!((b.fire - 5.0).abs() < 1e-9);
        assert!((b.wind - 6.0).abs() < 1e-9);
        assert_eq!(b.object, "test-obj");
    }

    #[test]
    fn learn_single_sample_identity() {
        let b = BaselineField::learn(&[[0.5, 0.6, 0.7, 0.8]], "o");
        assert!((b.to_array()[0] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let b = BaselineField { earth: 1.1, water: 0.9, fire: 2.0, wind: 1.0, object: "n", established: "learned" };
        let j = b.to_json();
        assert!(j.contains("\"earth\":1.1"), "{j}");
        let back = BaselineField::from_json(&j).expect("可解码");
        assert!((back.earth - 1.1).abs() < 1e-9);
        assert!((back.wind - 1.0).abs() < 1e-9);
    }

    #[test]
    fn adaptive_baseline_from_samples() {
        let samples = [
            [1.0, 1.0, 1.0, 1.0],
            [1.2, 0.9, 1.1, 1.0],
            [0.9, 1.1, 1.0, 1.2],
            [1.1, 1.0, 1.2, 0.95],
        ];
        let (b, spread) = BaselineField::adaptive_baseline(&samples);
        assert!((b.earth - 1.05).abs() < 1e-9 || (b.earth - 1.0).abs() < 0.11, "中位数附近: {}", b.earth);
        assert!(spread > 0.0 && spread < 1.0, "spread 合理: {spread}");
        assert_eq!(b.established, "derived");
    }

    #[test]
    fn fields_order_matches_senses() {
        assert_eq!(FIELDS, ["earth", "water", "fire", "wind"]);
    }
}
