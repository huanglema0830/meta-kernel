//! DNA 内核 · 自适应生成器（dna_generate）。
//!
//! 匹配失败时**生长**：试探扰动 → 观察响应 → 归纳规律 → 生成适配器 → 验证 → 存入痕迹库。
//! 纯逻辑（零依赖）：采样对（刺激, 响应）→ 归纳规则（线性/阈值/直通）→ 用测试集验证。

/// 一次试探：刺激 → 响应。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeResult {
    pub stimulus: f64,
    pub response: f64,
}

/// 归纳出的规律（生成的适配器核心）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    /// 线性响应：response ≈ a·stimulus + b。
    Linear { a: f64, b: f64 },
    /// 阈值响应：刺激落在 [lo, hi] 内输出 action（0/1 型行为）。
    Threshold { lo: f64, hi: f64, action: f64 },
    /// 直通（响应与刺激无关或恒等）。
    PassThrough,
}

/// 生成的适配器。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeneratedAdapter {
    pub id: u32,
    pub rule: Rule,
    pub signature: [f64; 7],
    pub verified: bool,
    /// 归纳样本数（学习规模）。
    pub learned_from: usize,
}

impl Rule {
    /// 预测响应（验证用）。
    pub fn predict(&self, stimulus: f64) -> f64 {
        match *self {
            Rule::Linear { a, b } => a * stimulus + b,
            Rule::Threshold { lo, hi, action } => {
                if stimulus >= lo && stimulus <= hi {
                    action
                } else {
                    0.0
                }
            }
            Rule::PassThrough => stimulus,
        }
    }
}

/// 归纳：由采样对推断规律。
/// - 响应方差极小 → PassThrough（恒定/无关）；
/// - 呈两值平台（阶跃）→ Threshold；
/// - 否则最小二乘线性拟合。
pub fn induce(samples: &[ProbeResult]) -> Rule {
    if samples.is_empty() {
        return Rule::PassThrough;
    }
    let n = samples.len() as f64;
    let mean_r = samples.iter().map(|p| p.response).sum::<f64>() / n;
    let var_r = samples.iter().map(|p| (p.response - mean_r).powi(2)).sum::<f64>() / n;
    if var_r < 1e-9 {
        return Rule::PassThrough;
    }
    // 两值平台检测：响应只取两个显著值
    let mut vals: Vec<f64> = samples.iter().map(|p| p.response).collect();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let lo_v = vals[0];
    let hi_v = vals[vals.len() - 1];
    let mid = (lo_v + hi_v) / 2.0;
    let distinct_low = samples.iter().filter(|p| (p.response - lo_v).abs() < 1e-9).count();
    let distinct_high = samples.iter().filter(|p| (p.response - hi_v).abs() < 1e-9).count();
    if samples.len() >= 3 && distinct_low + distinct_high == samples.len() && (hi_v - lo_v).abs() > 1e-9 {
        // 阈值区间：找到 action 对应的刺激范围
        let action = hi_v;
        let inside: Vec<f64> = samples
            .iter()
            .filter(|p| (p.response - action).abs() < 1e-9)
            .map(|p| p.stimulus)
            .collect();
        if !inside.is_empty() {
            let lo = inside.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = inside.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            return Rule::Threshold { lo, hi, action };
        }
        let _ = mid;
    }
    // 最小二乘线性拟合
    let mean_s = samples.iter().map(|p| p.stimulus).sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for p in samples {
        num += (p.stimulus - mean_s) * (p.response - mean_r);
        den += (p.stimulus - mean_s).powi(2);
    }
    if den.abs() < 1e-12 {
        return Rule::PassThrough;
    }
    let a = num / den;
    let b = mean_r - a * mean_s;
    Rule::Linear { a, b }
}

/// 生成适配器：归纳 + 携带签名（入库前）。
pub fn generate(samples: &[ProbeResult], signature: [f64; 7], next_id: u32) -> GeneratedAdapter {
    GeneratedAdapter {
        id: next_id,
        rule: induce(samples),
        signature,
        verified: false,
        learned_from: samples.len(),
    }
}

/// 验证：用未参与归纳的测试样本预测，误差 ≤ tol 则通过。
pub fn verify(adapter: &GeneratedAdapter, tests: &[ProbeResult], tol: f64) -> bool {
    if tests.is_empty() {
        return false;
    }
    tests.iter().all(|t| (adapter.rule.predict(t.stimulus) - t.response).abs() <= tol)
}

/// 生长主流程：生成 → 验证（可选）→ 返回成品（verified 标志）。
pub fn grow(samples: &[ProbeResult], tests: &[ProbeResult], signature: [f64; 7], next_id: u32, tol: f64) -> GeneratedAdapter {
    let mut a = generate(samples, signature, next_id);
    a.verified = verify(&a, tests, tol);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn induce_linear_rule() {
        // response = 2 x + 1
        let s = [
            ProbeResult { stimulus: 1.0, response: 3.0 },
            ProbeResult { stimulus: 2.0, response: 5.0 },
            ProbeResult { stimulus: 3.0, response: 7.0 },
            ProbeResult { stimulus: 4.0, response: 9.0 },
        ];
        match induce(&s) {
            Rule::Linear { a, b } => {
                assert!((a - 2.0).abs() < 1e-9, "a={a}");
                assert!((b - 1.0).abs() < 1e-9, "b={b}");
            }
            other => panic!("应为线性: {other:?}"),
        }
    }

    #[test]
    fn induce_threshold_rule() {
        // 刺激 5..7 输出 1，其他 0
        let s = [
            ProbeResult { stimulus: 1.0, response: 0.0 },
            ProbeResult { stimulus: 5.0, response: 1.0 },
            ProbeResult { stimulus: 6.0, response: 1.0 },
            ProbeResult { stimulus: 7.0, response: 1.0 },
            ProbeResult { stimulus: 9.0, response: 0.0 },
        ];
        match induce(&s) {
            Rule::Threshold { lo, hi, action } => {
                assert!((lo - 5.0).abs() < 1e-9 && (hi - 7.0).abs() < 1e-9 && (action - 1.0).abs() < 1e-9);
            }
            other => panic!("应为阈值: {other:?}"),
        }
    }

    #[test]
    fn induce_passthrough_for_constant_response() {
        let s = [
            ProbeResult { stimulus: 1.0, response: 0.5 },
            ProbeResult { stimulus: 2.0, response: 0.5 },
        ];
        assert_eq!(induce(&s), Rule::PassThrough);
    }

    #[test]
    fn grow_verifies_and_flags() {
        let learn = [
            ProbeResult { stimulus: 1.0, response: 3.0 },
            ProbeResult { stimulus: 2.0, response: 5.0 },
            ProbeResult { stimulus: 3.0, response: 7.0 },
        ];
        let tests = [
            ProbeResult { stimulus: 4.0, response: 9.0 },
            ProbeResult { stimulus: 5.0, response: 11.0 },
        ];
        let a = grow(&learn, &tests, [1.0; 7], 1, 1e-6);
        assert!(a.verified, "线性规律应验证通过");
        assert_eq!(a.learned_from, 3);
        // 反例：验证集不符 → 不通过（需更多采样）
        let bad_tests = [ProbeResult { stimulus: 4.0, response: 0.0 }];
        let b = grow(&learn, &bad_tests, [1.0; 7], 2, 1e-6);
        assert!(!b.verified);
    }

    #[test]
    fn empty_samples_safe() {
        assert_eq!(induce(&[]), Rule::PassThrough);
        let a = grow(&[], &[], [0.0; 7], 1, 0.1);
        assert!(!a.verified, "无测试集不得标记已验证");
    }
}
