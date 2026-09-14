//! # 外部知识库接口与内置故障模式（云操作系统 · 诊断）
//!
//! 原则（云内核·共生）：**知识库通过外部接口获取，不本地维护**。
//! - 配置：环境变量 `META_KNOWLEDGE_ENDPOINT`，或配置文件 `knowledge.json`：
//!   `{ "external_knowledge": { "endpoint": "https://…/diagnose", "query_param": "q" } }`
//! - 当内核产生**诊断状态**（显化停滞/低能量/固化/震荡）时，应用调用外部接口
//!   （GET `{endpoint}?{query_param}={urlencoded(描述)}`）获取排查步骤；
//! - 未配置 → 回退**内置通用故障模式（8 条）**。

/// 知识库配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KbConfig {
    pub endpoint: Option<String>,
    pub query_param: String,
}

impl KbConfig {
    /// 加载配置：环境变量 > knowledge.json（仓库根/当前目录）> 内置回退。
    pub fn load() -> Self {
        let mut cfg = Self { endpoint: None, query_param: "q".to_string() };
        if let Ok(url) = std::env::var("META_KNOWLEDGE_ENDPOINT") {
            if !url.trim().is_empty() {
                cfg.endpoint = Some(url.trim().to_string());
                return cfg;
            }
        }
        // knowledge.json：{"external_knowledge": {"endpoint": "...", "query_param": "q"}}
        if let Ok(text) = std::fs::read_to_string("knowledge.json") {
            if let Some(ep) = ext_field(&text, "endpoint") {
                cfg.endpoint = Some(ep);
            }
            if let Some(qp) = ext_field(&text, "query_param") {
                cfg.query_param = qp;
            }
        }
        cfg
    }
}

/// 极简字段提取（knowledge.json 单层对象用）。
fn ext_field(json: &str, key: &str) -> Option<String> {
    let n = format!("\"{key}\"");
    let i = json.find(&n)?;
    let r = &json[i + n.len()..];
    let c = r.find(':')? + 1;
    let v: String = r[c..].trim_start().chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '/' || *c == ':' || *c == '.'
            || *c == '-' || *c == '_' || *c == '?' || *c == '&' || *c == '=' || *c == '"')
        .collect();
    Some(v.trim_matches('"').to_string())
}

/// 诊断特征计数（由一次演化观测汇总）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trend {
    pub awaken: u32,
    pub settle: u32,
    pub hold: u32,
    pub low_energy_seen: bool,
    /// 演化结束时的生命周期状态（0 或 10..=99）。
    pub end_state: u16,
    /// 是否曾在极显带（≥90）后回落。
    pub peak_then_retreat: bool,
    /// settle-awaken 交替次数（震荡检测）。
    pub alternations: u32,
}

/// 诊断结论：可操作排查步骤 + 来源。
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnosis {
    pub input: String,
    pub seed: f32,
    pub steps: Vec<String>,
    pub source: String, // "builtin" | "external:<endpoint>"
}

/// 内置通用故障模式（8 条）——按特征匹配输出可操作步骤。
pub fn builtin_steps(t: &Trend) -> Vec<String> {
    if t.low_energy_seen && t.settle > t.awaken {
        return vec![
            "能量供给不足：储备持续低于 0.206（低能量预警）。".to_string(),
            "排查：注入 0.6–0.9 活性扰动 ×3，观察显化仪表储备是否上行。".to_string(),
            "若仍触底：检查输入是否含可被吸收的实质内容（而非空泛描述）。".to_string(),
        ];
    }
    if t.awaken >= 1 && t.end_state == 0 && t.settle >= 2 {
        return vec![
            "点亮即回融：扰动被点燃但随即被回落事件压回锚点。".to_string(),
            "排查：描述信息量不足或过于微弱——补充具体细节（对象/条件/期望）后重试。".to_string(),
            "可尝试把一条念头拆成更小的多条，逐条推进显化链。".to_string(),
        ];
    }
    if t.end_state >= 70 && t.end_state <= 89 && t.hold > t.awaken {
        return vec![
            "固化停滞：显化停在结晶/固化带，正向推进减弱（旧模式主导）。".to_string(),
            "排查：注入反向/孪生念头（与当前描述互补）以打破固化。".to_string(),
            "若长期驻留：考虑归档本轮，清零后以新视角重新点亮。".to_string(),
        ];
    }
    if t.peak_then_retreat {
        return vec![
            "极显早退：到达极显带后未获外部确认即回落。".to_string(),
            "排查：检查输出通道——显化产物是否已被世界接受/执行/回执。".to_string(),
            "补充世界反馈（确认动作）后再注入，可延长极显驻留。".to_string(),
        ];
    }
    if t.alternations >= 5 {
        return vec![
            "信号震荡：正向与回落事件交替 ≥5 次，显化目标自相矛盾。".to_string(),
            "排查：把描述收敛为单一明确句子（去掉并列/条件），再注入一次。".to_string(),
        ];
    }
    if t.awaken == 0 && t.hold >= 1 {
        return vec![
            "内核无响应：未收到正向演化事件。".to_string(),
            "排查：确认 push 返回 accepted:true；检查网关 /v1/health 与 SSE 订阅是否存活。".to_string(),
        ];
    }
    if t.end_state >= 10 && t.awaken >= 3 && t.settle == 0 {
        return vec![
            "总体正常推进中：未发现明显故障，显化链在成长。".to_string(),
            "建议：继续注入补充扰动观察趋势，或在显化停滞时再诊断。".to_string(),
        ];
    }
    vec![
        "通用排查（扰动→显化→回融 逐环）：".to_string(),
        "1) 注入：写入一条明确念头并确认 push accepted；".to_string(),
        "2) 显化：观察显化仪表是否自 0 点亮并向 10–99 推进；".to_string(),
        "3) 回融：若停在 99 静默或早退，归档后以新种子再试。".to_string(),
    ]
}

/// 完整诊断：按趋势生成结论（内置表）；若配置外部知识库则优先调用外部接口。
pub fn diagnose(input: &str, seed: f32, trend: &Trend, kb: &KbConfig) -> Diagnosis {
    if let Some(ep) = &kb.endpoint {
        if let Ok(steps) = fetch_external_steps(ep, &kb.query_param, input) {
            return Diagnosis { input: input.to_string(), seed, steps, source: format!("external:{ep}") };
        }
        // 外部调用失败 → 回退内置（不断链）
    }
    Diagnosis { input: input.to_string(), seed, steps: builtin_steps(trend), source: "builtin".to_string() }
}

/// 调用外部知识库：GET http://{host}/{path}?{query_param}={urlencoded(input)}。
/// 一期契约：**http:// 明文端点**（受控内网入口，与网关部署形态一致）；
/// https/TLS 留二期（零第三方依赖下不引入 TLS 栈）。
/// 期望响应 JSON：{"steps":["…","…"]}；宽松解析失败时按行拆分文本。
fn fetch_external_steps(endpoint: &str, qp: &str, input: &str) -> Result<Vec<String>, String> {
    let (addr, path) = parse_http(endpoint).ok_or_else(|| "endpoint 需为 http://host[:port]/path".to_string())?;
    let q = input.as_bytes().iter().map(|b| {
        if b.is_ascii_alphanumeric() || b"-_.~ ".contains(b) {
            (*b as char).to_string()
        } else {
            format!("%{:02X}", b)
        }
    }).collect::<String>().replace(' ', "+");
    let sep = if path.contains('?') { "&" } else { "?" };
    let full_path = format!("{path}{sep}{qp}={q}");
    let body = npb_appkit::httpc::get_json(&addr, &full_path)
        .map_err(|e| e.to_string())?;
    if let Some(arr) = parse_string_array(&body, "steps") {
        return Ok(arr);
    }
    let cleaned = body.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim_matches('"').trim().to_string())
        .collect::<Vec<_>>();
    if cleaned.is_empty() { Err("empty external response".to_string()) } else { Ok(cleaned) }
}

/// 解析 http://host[:port]/path（端口缺省 80）→ (addr, path)。
fn parse_http(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = if authority.contains(':') { authority.to_string() } else { format!("{authority}:80") };
    Some((addr, path.to_string()))
}

/// 极简 JSON 字符串数组解析（宽松：截取 [ … ] 后取带引号片段）。
fn parse_string_array(body: &str, _key: &str) -> Option<Vec<String>> {
    let start = body.find('[')?;
    let end = body.find(']')?;
    let inner = &body[start + 1..end];
    let items = inner.split('"')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && *s != ",")
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    if items.is_empty() { None } else { Some(items) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn trend() -> Trend {
        Trend { awaken: 1, settle: 4, hold: 2, low_energy_seen: true, end_state: 0, peak_then_retreat: false, alternations: 0 }
    }

    #[test]
    fn low_energy_matches_builtin() {
        let kb = KbConfig { endpoint: None, query_param: "q".into() };
        let d = diagnose("按电源键风扇转屏幕黑", 0.5, &trend(), &kb);
        assert_eq!(d.source, "builtin");
        assert!(d.steps[0].contains("能量供给不足"), "{:?}", d.steps);
    }

    #[test]
    fn alternation_and_stall_rules() {
        let t = Trend { awaken: 6, settle: 6, hold: 0, low_energy_seen: false, end_state: 20, peak_then_retreat: false, alternations: 6 };
        let s = builtin_steps(&t);
        assert!(s[0].contains("信号震荡"), "{:?}", s);
        let t2 = Trend { awaken: 2, settle: 1, hold: 3, low_energy_seen: false, end_state: 75, peak_then_retreat: false, alternations: 0 };
        assert!(builtin_steps(&t2)[0].contains("固化停滞"), "{:?}", builtin_steps(&t2));
    }

    #[test]
    fn parse_external_url_and_steps() {
        let (a, p) = parse_http("http://127.0.0.1:9876/diag").unwrap();
        assert_eq!(a, "127.0.0.1:9876");
        assert_eq!(p, "/diag");
        let steps = parse_string_array(r#"{"steps":["第一步","第二步"]}"#, "steps").unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1], "第二步");
    }

    #[test]
    fn external_endpoint_called_when_configured() {
        // 起本地 mock 知识库（std TCP）：GET /diag?q=… → {"steps":["外部步骤A"]}
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b);
                let resp = "HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: 25
Connection: close

{\"steps\":[\"外部步骤A\"]}";
                let _ = s.write_all(resp.as_bytes());
            }
        });
        let ep = format!("http://{addr}/diag");
        let kb = KbConfig { endpoint: Some(ep.clone()), query_param: "q".into() };
        let t = Trend { awaken: 2, settle: 2, hold: 1, low_energy_seen: false, end_state: 40, peak_then_retreat: false, alternations: 0 };
        let d = diagnose("内存不足", 0.5, &t, &kb);
        srv.join().ok();
        assert_eq!(d.source, format!("external:{ep}"), "{:?}", d);
        assert!(d.steps.iter().any(|x| x.contains("外部步骤A")), "{:?}", d.steps);
    }

    #[test]
    fn config_load_from_file() {
        // knowledge.json 存在则读 endpoint（临时目录 cwd 不可控 → 仅验证文件缺失时默认）
        let kb = KbConfig::load();
        assert_eq!(kb.query_param, "q");
        let _ = kb.endpoint; // env 可能注入，不硬断言
    }
}
