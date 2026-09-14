//! # HTTP/SSE 服务器层（npb-gateway，一期传输）
//!
//! std::net 手写 HTTP/1.1 极小服务器：GET/POST 路由 + SSE 长连接。
//! - 每连接一线程；内核访问一律经 [`crate::Gateway`]（内部 mpsc→kern 线程串行）——多线程安全；
//! - SSE 连接线程只读共享投影，经 [`crate::edges`] 产出 state_change，消费式转发指令；
//!   连接建立即推一条 snapshot 供秒同步；低频轮询（100ms）+ 心跳 ping，不空转驱动内核；
//! - 路由（协议基线 v1.0）：POST /v1/push｜GET /v1/state｜GET /v1/events｜
//!   POST /v1/persist/snapshot 与 /v1/persist/restore（kern 线程直通 npb persist_*）｜
//!   GET /v1/health（含单写者语义声明）。
//!
//! 单写者语义：任何 HTTP 线程都不直接触碰 npb；所有内核操作经 Gateway 串行。

use crate::{edges, health_json, parse_seed_body, Gateway, Projection};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 运行中的服务器实例。
pub struct Server {
    pub addr: String,
    /// 可选静态 UI 目录（--ui）：GET / 及受控静态文件由网关同源托管（老设备一键部署）。
    ui_dir: Option<std::path::PathBuf>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

fn serve_forever(listener: TcpListener, gw: Arc<Gateway>, stop: Arc<AtomicBool>, ui_dir: Option<std::path::PathBuf>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(s) => {
                let gw2 = Arc::clone(&gw);
                let stop2 = Arc::clone(&stop);
                let ui2 = ui_dir.clone();
                std::thread::spawn(move || {
                    let _ = handle_conn(s, gw2, stop2, ui2);
                });
            }
            Err(_) => break,
        }
    }
}

/// 绑定 127.0.0.1:port（0 = 随机可用端口）并启动服务器线程。
pub fn spawn(port: u16) -> std::io::Result<Server> {
    spawn_custom(port, None)
}

/// 按端口（127.0.0.1）启动；`ui_dir` 提供时同源托管静态 UI（老设备一键部署）。
pub fn spawn_custom(port: u16, ui_dir: Option<String>) -> std::io::Result<Server> {
    spawn_on("127.0.0.1", port, ui_dir)
}

/// 指定监听地址（局域网实测：如 "192.168.1.3"——受控内网入口；默认仍 127.0.0.1）。
pub fn spawn_on(ip: &str, port: u16, ui_dir: Option<String>) -> std::io::Result<Server> {
    let listener = TcpListener::bind((ip, port))?;
    let addr = listener.local_addr()?.to_string();
    let gw = Arc::new(Gateway::spawn());
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = Arc::clone(&stop);
    let gw2 = Arc::clone(&gw);
    let ui = ui_dir.map(std::path::PathBuf::from);
    let ui_ok = ui.clone();
    let handle = std::thread::spawn(move || serve_forever(listener, gw2, s2, ui));
    Ok(Server { addr, ui_dir: ui_ok, stop, handle: Some(handle) })
}

impl Server {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(&self.addr);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

const CORS: &str = "Access-Control-Allow-Origin: *";

fn http_ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{CORS}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn http_err(code: u16, reason: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\n{CORS}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code,
        reason,
        body.len(),
        body
    )
}

/// 纯文本 200（报告 / 脚本下载；避免浏览器按 JSON 处理）。
fn http_ok_plain(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\n{CORS}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// HTML 200（升级页等）。
fn http_ok_html(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n{CORS}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// 能力特征（**版本无关的升级判据**：老笔记本据此确认已升到含这些能力的版本）。
const FEATURES: &str = "report,alerts,tasks,upgrade,compat,workbench";

/// 宽松提取 JSON 字符串字段（仅用于把外部投递的文本读出来记录，不参与判定）。
/// 支持 `\"` `\\` `\n` `\r` `\t` 与 **`\uXXXX`**（CJK 常以 `\uXXXX` 传输）。
fn extract_json_str(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let i = body.find(&pat)?;
    let rest = &body[i + pat.len()..];
    let c = rest.find(':')? + 1;
    let rest = &rest[c..];
    let start = rest.find('"')? + 1;
    let rest = &rest[start..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push(' '),
                Some('r') | Some('t') => out.push(' '),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if hex.len() == 4 {
                        if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                            if let Some(c) = char::from_u32(cp) {
                                out.push(c);
                            }
                        }
                    }
                }
                Some(other) => out.push(other),
                None => break,
            },
            other => out.push(other),
        }
    }
    Some(out)
}

/// CORS 预检响应（浏览器跨源调用 /v1/*；受信本机/内网一期放开）。
fn http_options() -> String {
    format!(
        "HTTP/1.1 204 No Content\r\n{CORS}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

fn read_line(stream: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<Option<String>> {
    loop {
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let mut s = String::from_utf8_lossy(&line).into_owned();
            s = s.trim_end_matches(['\r', '\n']).to_string();
            return Ok(Some(s));
        }
        let mut chunk = [0u8; 256];
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// 读取请求行 + 头，返回 (请求行, 头列表, 已读入的多余字节[body 起始])。
/// 注意：读取 chunk 可能把 body 一并读入——多余字节必须返回给调用方消费，否则 body 丢失。
fn read_request(stream: &mut TcpStream) -> (String, Vec<String>, Vec<u8>) {
    let mut buf: Vec<u8> = Vec::new();
    let mut request_line = String::new();
    let mut headers = Vec::new();
    loop {
        match read_line(stream, &mut buf) {
            Ok(Some(line)) => {
                if line.is_empty() {
                    break; // 空行 = 头结束
                }
                if request_line.is_empty() {
                    request_line = line;
                } else {
                    headers.push(line);
                }
            }
            _ => break,
        }
    }
    (request_line, headers, buf)
}

fn content_length(headers: &[String]) -> usize {
    for h in headers {
        let low = h.to_ascii_lowercase();
        if let Some(v) = low.strip_prefix("content-length:") {
            return v.trim().parse::<usize>().unwrap_or(0);
        }
    }
    0
}

fn handle_conn(mut stream: TcpStream, gw: Arc<Gateway>, stop: Arc<AtomicBool>, ui_dir: Option<std::path::PathBuf>) -> std::io::Result<()> {
    let (request_line, headers, mut leftover) = read_request(&mut stream);
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 3 {
        return Ok(());
    }
    let method = parts[0];
    let target = parts[1];

    // ---- CORS 预检 ----
    if method == "OPTIONS" {
        stream.write_all(http_options().as_bytes())?;
        return Ok(());
    }

    // ---- SSE 订阅（长连接；先推 snapshot 秒同步，后按边沿/指令推送） ----
    if method == "GET" && target == "/v1/events" {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: keep-alive\r\n\r\n",
        )?;
        write_sse(&mut stream, "snapshot", &gw.snapshot_json())?;
        let mut prev: Option<Projection> = None;
        let mut beats: u64 = 0;
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let now = gw.projection();
            if let Some(p) = &prev {
                for e in edges(p, &now) {
                    let ev = format!(
                        "{{\"field\":\"{f}\",\"from\":{from},\"to\":{to}}}",
                        f = e.field,
                        from = e.from,
                        to = e.to
                    );
                    write_sse(&mut stream, "state_change", &ev)?;
                }
            }
            prev = Some(now);
            // 指令消费式转发（一期推荐单订阅者；多订阅者竞争留待二期广播）
            for ins in gw.take_instructions() {
                write_sse(&mut stream, "instruction", &ins)?;
            }
            beats += 1;
            if beats % 20 == 0 {
                write_sse(&mut stream, "ping", "{}")?;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        return Ok(());
    }

    // ---- 读 body（POST）：先消费请求头读取时已带入的多余字节，再补齐剩余 ----
    let mut body = String::new();
    if method == "POST" {
        let clen = content_length(&headers);
        if clen > 0 {
            let mut body_bytes: Vec<u8> = Vec::with_capacity(clen);
            let take = leftover.len().min(clen);
            body_bytes.extend(leftover.drain(..take));
            let need = clen - body_bytes.len();
            if need > 0 {
                let mut v = vec![0u8; need];
                let _ = stream.read_exact(&mut v);
                body_bytes.extend(v);
            }
            body = String::from_utf8_lossy(&body_bytes).into_owned();
        }
    }

    // ---- 元内核接管（立即移交）：自我监控 / 健康报告 / 异常告警 / 运行日志（含任务归属）----
    if target.starts_with("/v1/") {
        gw.mon().count_request();
    }
    if method == "GET" && target == "/v1/report" {
        stream.write_all(http_ok(&gw.mon().report_json(env!("CARGO_PKG_VERSION"), FEATURES)).as_bytes())?;
        return Ok(());
    }
    if method == "GET" && target == "/v1/report.txt" {
        stream.write_all(http_ok_plain(&gw.mon().report_txt(env!("CARGO_PKG_VERSION"), FEATURES)).as_bytes())?;
        return Ok(());
    }
    if method == "GET" && target == "/v1/alerts" {
        stream.write_all(http_ok(&gw.mon().alerts_json()).as_bytes())?;
        return Ok(());
    }
    if method == "GET" && target == "/v1/tasks" {
        stream.write_all(http_ok(&gw.mon().tasks_json(200)).as_bytes())?;
        return Ok(());
    }
    if method == "GET" && target == "/v1/tasks.txt" {
        stream.write_all(http_ok_plain(&gw.mon().tasks_txt(200)).as_bytes())?;
        return Ok(());
    }
    // 外部投递任务（默认归属 [WorkBuddy]；体可为 JSON{owner,detail} 或纯文本）
    if method == "POST" && target == "/v1/tasks" {
        let detail = extract_json_str(&body, "detail").unwrap_or_else(|| body.trim().to_string());
        let owner = match extract_json_str(&body, "owner") {
            Some(o) => crate::selfmon::Owner::parse(&o),
            None => crate::selfmon::Owner::WorkBuddy,
        };
        gw.mon().count_task_post();
        let l = gw.mon().note(
            owner,
            "TASK",
            "任务投递",
            if detail.is_empty() { "（无明细）".to_string() } else { detail },
        );
        stream.write_all(http_ok(&format!("{{\"accepted\":true,\"seq\":{}}}", l.seq)).as_bytes())?;
        return Ok(());
    }

    // ---- 升级入口（老笔记本升级）----
    // 只替换**本应用的文件**；脚本内不触碰任何网络配置（IP/DNS/代理/hosts/防火墙/路由）。
    if method == "GET" && (target == "/upgrade" || target == "/upgrade/") {
        let lport = stream.local_addr().map(|a| a.port()).unwrap_or(0);
        let origin = request_origin(&headers, lport);
        stream.write_all(http_ok_html(&upgrade_page(&origin)).as_bytes())?;
        return Ok(());
    }
    if method == "GET" && target == "/upgrade.bat" {
        let lport = stream.local_addr().map(|a| a.port()).unwrap_or(0);
        let origin = request_origin(&headers, lport);
        stream.write_all(http_ok_plain(&upgrade_bat(&origin)).as_bytes())?;
        return Ok(());
    }

    // ---- 静态 UI（同源托管：--ui 提供时 GET / 与受控静态文件） ----
    if method == "GET" {
        let lport = stream.local_addr().map(|a| a.port()).unwrap_or(0);
        if let Some(resp) = serve_ui(&ui_dir, target, &headers, lport) {
            stream.write_all(&resp)?;
            return Ok(());
        }
    }

    if method == "POST" && target == "/v1/probe" {
        gw.store_probe(body.clone());
        gw.mon().count_probe();
        gw.mon().note(crate::selfmon::Owner::Kernel, "INFO", "场域探针上报", format!("len={}", body.len()));
        let ok = http_ok(&format!("{{\"probe_accepted\":true,\"len\":{}}}", body.len()));
        stream.write_all(ok.as_bytes())?;
        return Ok(());
    }
    if method == "GET" && target == "/v1/probe" {
        stream.write_all(http_ok(&gw.latest_probe_json()).as_bytes())?;
        return Ok(());
    }
    // ---- USB 场检测（探针扩展；同 probe：只存取不解释）----
    if method == "POST" && target == "/v1/probe/usb" {
        gw.store_usb(body.clone());
        let ok = http_ok(&format!("{{\"usb_accepted\":true,\"len\":{}}}", body.len()));
        stream.write_all(ok.as_bytes())?;
        return Ok(());
    }
    if method == "GET" && target == "/v1/probe/usb" {
        stream.write_all(http_ok(&gw.latest_usb_json()).as_bytes())?;
        return Ok(());
    }

    let resp = match (method, target) {
        // 注入扰动：外部 push 驱动内核（网关不空转）
        ("POST", "/v1/push") => match parse_seed_body(&body) {
            Some(seed) => {
                let ok = gw.push(seed);
                gw.mon().count_push(ok);
                gw.mon().note(
                    crate::selfmon::Owner::Kernel,
                    if ok { "INFO" } else { "WARN" },
                    "内核推注",
                    format!("seed={seed:.4} -> {}", if ok { "accepted" } else { "gate_rejected" }),
                );
                if ok {
                    let t = gw.projection().t;
                    http_ok(&format!("{{\"accepted\":true,\"t\":{t},\"tag\":null}}"))
                } else {
                    http_ok("{\"accepted\":false,\"reason\":\"gate_rejected\",\"tag\":null}")
                }
            }
            None => {
                gw.mon().count_error();
                gw.mon().note(crate::selfmon::Owner::Kernel, "WARN", "推注被拒", "缺少 seed 字段");
                http_err(400, "Bad Request", "{\"error\":\"seed_required\"}")
            }
        },
        // 当前内核快照
        ("GET", "/v1/state") => http_ok(&gw.snapshot_json()),
        // 持久化直通（kern 线程）
        ("POST", "/v1/persist/snapshot") => http_ok(&gw.persist_snapshot()),
        ("POST", "/v1/persist/restore") => {
            let ok = gw.persist_restore(&body);
            http_ok(&format!("{{\"ok\":{}}}", if ok { "true" } else { "false" }))
        }
        // 健康 + 单写者语义声明
        ("GET", "/v1/health") => http_ok(&health_json()),
        _ => http_err(404, "Not Found", "{\"error\":\"not_found\"}"),
    };
    stream.write_all(resp.as_bytes())?;
    Ok(())
}

fn write_sse(stream: &mut TcpStream, event: &str, data: &str) -> std::io::Result<()> {
    let frame = format!("event: {event}\ndata: {data}\n\n");
    stream.write_all(frame.as_bytes())
}

#[cfg(test)]

#[cfg(test)]
mod http_tests {
    use super::*;
    use std::io::BufRead;
    use std::io::BufReader;

    /// 带 3s 读超时的原始 HTTP 请求：返回全部响应字节（EOF 或超时止），杜绝无限挂。
    fn raw_request(addr: &str, req: &str) -> String {
        let mut s = TcpStream::connect(addr).expect("connect");
        s.set_read_timeout(Some(Duration::from_millis(3000))).ok();
        let _ = s.write_all(req.as_bytes());
        let mut out = String::new();
        let mut b = [0u8; 1024];
        loop {
            match s.read(&mut b) {
                Ok(0) => break,
                Ok(n) => out.push_str(&String::from_utf8_lossy(&b[..n])),
                Err(_) => break,
            }
        }
        out
    }

    /// POST helper：Content-Length 动态取自 body 实际长度。
    fn post_json(addr: &str, path: &str, body: &str) -> String {
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        raw_request(addr, &req)
    }

    #[test]
    fn state_returns_snapshot_json() {
        let mut srv = spawn(0).expect("spawn");
        let body = raw_request(&srv.addr, "GET /v1/state HTTP/1.1\r\nHost: x\r\n\r\n");
        srv.stop();
        assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
        assert!(body.contains("\"schema\":1"));
        assert!(body.contains("\"extensions\":{}"));
        assert!(body.contains("\"energy\":{\"stored\":"));
    }

    #[test]
    fn push_accepts_positive_and_advances_tick() {
        let mut srv = spawn(0).expect("spawn");
        let ok = post_json(&srv.addr, "/v1/push", r#"{"seed": 0.5}"#);
        let st = raw_request(&srv.addr, "GET /v1/state HTTP/1.1\r\nHost: x\r\n\r\n");
        srv.stop();
        assert!(ok.contains("\"accepted\":true"), "{ok}");
        assert!(st.contains("\"t\":1"), "push 应推进 tick: {st}");
    }

    #[test]
    fn negative_push_rejected_with_reason() {
        let mut srv = spawn(0).expect("spawn");
        let bad = post_json(&srv.addr, "/v1/push", r#"{"seed": -0.25}"#);
        let st = raw_request(&srv.addr, "GET /v1/state HTTP/1.1\r\nHost: x\r\n\r\n");
        srv.stop();
        assert!(bad.contains("\"accepted\":false"), "{bad}");
        assert!(bad.contains("gate_rejected"), "{bad}");
        assert!(st.contains("\"t\":0"), "被拒不推进: {st}");
    }

    #[test]
    fn health_declares_digest_and_single_writer() {
        let mut srv = spawn(0).expect("spawn");
        let h = raw_request(&srv.addr, "GET /v1/health HTTP/1.1\r\nHost: x\r\n\r\n");
        srv.stop();
        assert!(h.contains("\"ok\":true"), "{h}");
        assert!(h.contains("\"writer\":\"single\""), "{h}");
        assert!(h.contains("\"digest\":"), "{h}");
    }

    #[test]
    fn probe_store_and_read_roundtrip() {
        let srv = spawn(0).expect("spawn");
        let addr = srv.addr.clone();
        let body = "{\"schema\":1,\"s\":[1.0;7],\"at\":\"t\"}";
        let post = raw_request(&addr, &format!("POST /v1/probe HTTP/1.1
Host: x
Content-Length: {}
Connection: close

{body}", body.len()));
        assert!(post.contains("probe_accepted"), "{post}");
        let get = raw_request(&addr, "GET /v1/probe HTTP/1.1
Host: x
Connection: close

");
        assert!(get.contains("\"schema\":1"), "{get}");
        let mut s2 = srv;
        s2.stop();
    }

    #[test]
    fn static_ui_served_same_origin() {
        // 临时 ui 目录：仅 index.html
        let dir = std::env::temp_dir().join(format!("ck_ui_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(dir.join("index.html"), "<html>cloud-kernel ui</html>").ok();
        let srv = spawn_custom(0, Some(dir.to_string_lossy().into_owned())).expect("spawn ui");
        let addr = srv.addr.clone();
        // ---- 动态 run-probe.bat：地址随访问 Host 变化（IP 漂移免疫） ----
        let bat_a = raw_request(&addr, "GET /run-probe.bat HTTP/1.1\r\nHost: 192.168.1.99:3000\r\nConnection: close\r\n\r\n");
        assert!(bat_a.contains("192.168.1.99:3000"), "bat 应按 Host 生成: {bat_a}");
        assert!(bat_a.contains("cloud-probe.exe"), "{bat_a}");
        let bat_b = raw_request(&addr, "GET /run-probe.bat HTTP/1.1\r\nHost: 10.0.0.5:3000\r\nConnection: close\r\n\r\n");
        assert!(bat_b.contains("10.0.0.5:3000"), "换 Host 应换地址: {bat_b}");
        assert!(!bat_b.contains("192.168.1.99"), "不得残留旧地址: {bat_b}");
        let idx = raw_request(&addr, "GET / HTTP/1.1
Host: x

");
        assert!(idx.contains("cloud-kernel ui"), "{idx}");
        // API 与静态共存
        let h = raw_request(&addr, "GET /v1/health HTTP/1.1
Host: x

");
        assert!(h.contains("\"ok\":true"), "{h}");
        // 白名单外 → 404（防穿越/外泄）
        let bad = raw_request(&addr, "GET /../../windows/win.ini HTTP/1.1
Host: x

");
        assert!(bad.starts_with("HTTP/1.1 404"), "{bad}");
        let mut srv2 = srv;
        srv2.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cors_preflight_and_headers_enabled() {
        let mut srv = spawn(0).expect("spawn");
        // OPTIONS 预检 → 204 + 允许方法/头
        let pre = raw_request(&srv.addr, "OPTIONS /v1/push HTTP/1.1\r\nHost: x\r\nAccess-Control-Request-Method: POST\r\n\r\n");
        srv.stop();
        assert!(pre.starts_with("HTTP/1.1 204"), "{pre}");
        assert!(pre.contains("Access-Control-Allow-Origin: *"), "{pre}");
        assert!(pre.contains("Access-Control-Allow-Methods"), "{pre}");
        // 普通响应也带 ACAO（浏览器跨源读）
        let mut srv2 = spawn(0).expect("spawn2");
        let st = raw_request(&srv2.addr, "GET /v1/state HTTP/1.1\r\nHost: x\r\nOrigin: http://localhost:8081\r\n\r\n");
        srv2.stop();
        assert!(st.contains("Access-Control-Allow-Origin: *"), "{st}");
    }

    #[test]
    fn sse_stream_emits_snapshot_then_events() {
        let srv = spawn(0).expect("spawn");
        let addr = srv.addr.clone();
        let reader = std::thread::spawn(move || {
            let s = TcpStream::connect(&addr).expect("sse connect");
            s.set_read_timeout(Some(Duration::from_millis(3000))).ok();
            let mut w = &s;
            let _ = w.write_all(b"GET /v1/events HTTP/1.1\r\nHost: x\r\n\r\n");
            let mut r = BufReader::new(s);
            let mut first = String::new();
            for _ in 0..8 {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                first.push_str(&line);
                if line.trim().is_empty() && first.contains("event:") {
                    break;
                }
            }
            first
        });
        std::thread::sleep(Duration::from_millis(300));
        let _ = post_json(&srv.addr, "/v1/push", r#"{"seed": 0.9}"#);
        let got = reader.join().unwrap_or_default();
        let mut srv2 = srv;
        srv2.stop();
        assert!(got.contains("event: snapshot"), "首帧应为 snapshot: {got}");
    }

    #[test]
    fn persist_roundtrip_via_http() {
        let mut srv = spawn(0).expect("spawn");
        let _ = post_json(&srv.addr, "/v1/push", r#"{"seed": 0.6}"#);
        let snap = post_json(&srv.addr, "/v1/persist/snapshot", "");
        srv.stop();
        assert!(snap.contains("HTTP/1.1 200"), "{snap}");
        assert!(snap.contains("stored") || snap.contains("\"self\""), "{snap}");
    }
}

/// 静态 UI 服务（同源托管，老设备一键部署）：GET / → index.html；其余仅白名单文件。
/// 白名单：index.html / manifest_ui.js / manifest_ui_bg.wasm（防止路径穿越与任意文件外泄）。
/// v0.101：新增网络排查/修复脚本（net-check.bat / net-repair.bat / net-diagnose.ps1 / net-repair.ps1），
/// 供老笔记本经局域网直接下载（地址随访问 Host 变化，无需知道开发机 IP 之外的任何信息）。
/// v0.102：新增单文件入口（net-check-one.bat / net-repair-one.bat）——自身下载依赖脚本并运行，
/// 用户只需「下载 1 个文件 → 双击」两步。
/// v0.105：新增兼容性测试页（compat-test.html）——在老笔记本上打开即出报告（先验证再开发）。
/// v0.106：新增升级包（upgrade-package.zip）——老笔记本经局域网一键升级（只换本应用文件）。
const UI_ALLOW: [(&str, &str); 14] = [
    ("/index.html", "text/html; charset=utf-8"),
    ("/manifest_ui.js", "text/javascript"),
    ("/manifest_ui_bg.wasm", "application/wasm"),
    ("/cloud-probe.exe", "application/octet-stream"),
    ("/cloud-discover.exe", "application/octet-stream"),
    ("/run-probe.bat", "text/plain; charset=utf-8"),
    ("/net-check.bat", "text/plain; charset=utf-8"),
    ("/net-repair.bat", "text/plain; charset=utf-8"),
    ("/net-diagnose.ps1", "text/plain; charset=utf-8"),
    ("/net-repair.ps1", "text/plain; charset=utf-8"),
    ("/net-check-one.bat", "text/plain; charset=utf-8"),
    ("/net-repair-one.bat", "text/plain; charset=utf-8"),
    ("/compat-test.html", "text/html; charset=utf-8"),
    ("/upgrade-package.zip", "application/zip"),
];

/// 由请求头取**访问来源 origin**（host[:port]），用于按访问者实际地址动态生成脚本。
/// 优先用 Host 头自带的端口（跨机/端口转发时才是用户真正可用的地址）；缺失则补监听端口。
fn request_origin(headers: &[String], listen_port: u16) -> String {
    for line in headers {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("Host:") {
            let hv = v.trim().to_string();
            if hv.is_empty() {
                continue;
            }
            let host_part = hv.rsplit(':').next().unwrap_or("");
            let has_port = hv.matches(':').count() == 1
                && !host_part.is_empty()
                && host_part.chars().all(|c| c.is_ascii_digit());
            if has_port || listen_port == 80 {
                return hv;
            }
            return format!("{hv}:{listen_port}");
        }
    }
    format!("127.0.0.1:{listen_port}")
}

/// 动态生成一键探针脚本：地址按**访问者实际使用的 origin** 生成——
/// 开发机 IP 变化（DHCP 重分配）不再导致目标机脚本失效。
fn run_probe_bat(origin: &str) -> Vec<u8> {
    let body = format!(
        "@echo off\r\n\
title Cloud Probe One-Click\r\n\
cd /d %~dp0\r\n\
echo [1/3] Downloading cloud-probe.exe from http://{origin} ...\r\n\
curl -s -f -o cloud-probe.exe \"http://{origin}/cloud-probe.exe\"\r\n\
if not exist cloud-probe.exe ( powershell -NoProfile -Command \"(New-Object Net.WebClient).DownloadFile('http://{origin}/cloud-probe.exe','cloud-probe.exe')\" )\r\n\
if not exist cloud-probe.exe ( echo [ERROR] download failed & pause & exit /b 1 )\r\n\
echo [2/3] Running probe and reporting to gateway ...\r\n\
cloud-probe.exe --report http://{origin}/v1/probe\r\n\
echo [3/3] Done. If you see \"probe posted\" above, success.\r\n\
pause\r\n"
    );
    body.into_bytes()
}

/// 升级页（GET /upgrade）：给老笔记本的**一键升级入口**。
/// 页面本身不做任何写操作；真正的写操作在 `upgrade.bat`，且**只替换本应用文件**。
fn upgrade_page(origin: &str) -> String {
    let mut h = String::new();
    h.push_str("<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    h.push_str("<title>升级 · 空天浏览器</title><style>");
    h.push_str("body{font-family:system-ui,\"Microsoft YaHei\",sans-serif;margin:0;background:#f6f6f8;color:#1e1e24}");
    h.push_str(".wrap{max-width:800px;margin:0 auto;padding:20px}h1{font-size:18px;margin:0 0 4px}");
    h.push_str(".sub{color:#6b6b78;font-size:12px;margin-bottom:14px}");
    h.push_str(".card{background:#fff;border:1px solid #e6e6ec;border-radius:12px;padding:14px;margin-bottom:12px}");
    h.push_str(".btn{display:inline-block;background:#2f6fb5;color:#fff;text-decoration:none;padding:10px 18px;border-radius:10px;font-weight:600}");
    h.push_str(".warn{background:#fff8e6;border:1px solid #f0d9a0;border-radius:10px;padding:10px 12px;font-size:13px;color:#7a5b12}");
    h.push_str("code{background:#f2f2f6;padding:1px 5px;border-radius:4px;font-size:12px}");
    h.push_str("ol{font-size:13px;line-height:1.9}li{margin-bottom:2px}ul{font-size:13px;line-height:1.8}");
    h.push_str("</style></head><body><div class=\"wrap\">");
    h.push_str("<h1>空天浏览器 · 升级入口</h1>");
    h.push_str("<div class=\"sub\">把老笔记本从旧版升级到新版（v0.105+）。<b>本升级不修改任何网络配置。</b></div>");

    h.push_str("<div class=\"card\"><div class=\"warn\"><b>安全承诺（与发起人约束一致）</b><ul>");
    h.push_str("<li>只替换<b>本应用的文件</b>（exe / ui / 脚本），<b>不触碰</b> IP、DNS、代理、hosts、防火墙、路由；</li>");
    h.push_str("<li><b>不设置任何系统/浏览器代理</b>；<b>不影响</b>老笔记本上运行的其他项目；</li>");
    h.push_str("<li>改动前先<b>自动备份</b>到 <code>_backup_&lt;时间戳&gt;\\</code>（可回滚）；</li>");
    h.push_str("<li>升级只涉及<b>本应用目录</b>，不动系统服务。</li>");
    h.push_str("</ul></div></div>");

    h.push_str("<div class=\"card\"><div style=\"font-weight:600;font-size:14px;margin-bottom:8px\">升级步骤（3 步）</div>");
    h.push_str("<ol>");
    h.push_str("<li>点下面按钮下载 <code>upgrade.bat</code>；</li>");
    h.push_str("<li>把它放到老笔记本的<b>部署包目录</b>（与 <code>npb-gateway.exe</code> 同一层）；</li>");
    h.push_str("<li>双击运行（会自动备份 → 下载升级包 → 停止本应用 → 解压覆盖 → 重启看门狗 → 自检）。</li>");
    h.push_str("</ol><p>");
    h.push_str("<a class=\"btn\" href=\"/upgrade.bat\">下载 upgrade.bat</a></p>");
    h.push_str("<div class=\"sub\">升级包来源：<code>");
    h.push_str(origin);
    h.push_str("/upgrade-package.zip</code></div></div>");

    h.push_str("<div class=\"card\"><div style=\"font-weight:600;font-size:14px;margin-bottom:8px\">升级后验证（4 项）</div>");
    h.push_str("<ul><li>页面标题为「<b>空天浏览器</b>」；</li>");
    h.push_str("<li>命名已更新（云操作系统 / 空天浏览器 / 云海大模型 等，见 <code>LAYER_ARCHITECTURE §3.2</code>）；</li>");
    h.push_str("<li><b>工作台可用</b>：写文档（Markdown + 实时预览）、文件管理；</li>");
    h.push_str("<li>资源占用：内存 &lt; 20 MB、启动 &lt; 1 秒（页面左下角状态带可见）。</li></ul>");
    h.push_str("<p class=\"sub\">自检入口：<code>");
    h.push_str(origin);
    h.push_str("/v1/report.txt</code>（元内核健康报告）</p></div>");

    h.push_str("</div></body></html>");
    h
}

/// 升级脚本（GET /upgrade.bat）：**只在老笔记本上替换本应用文件**；不碰任何网络配置。
fn upgrade_bat(origin: &str) -> String {
    let mut s = String::new();
    s.push_str("@echo off\r\n");
    s.push_str("chcp 65001 >nul\r\n");
    s.push_str("rem ============================================================\r\n");
    s.push_str("rem  空天浏览器 · 升级脚本（老笔记本用）\r\n");
    s.push_str("rem  安全承诺：只替换本应用文件；不修改 IP / DNS / 代理 / hosts / 防火墙 / 路由。\r\n");
    s.push_str("rem  用法：放到部署包目录（与 npb-gateway.exe 同层）→ 双击运行。\r\n");
    s.push_str("rem ============================================================\r\n");
    s.push_str("setlocal enabledelayedexpansion\r\n");
    s.push_str("set \"ORIGIN=");
    s.push_str(origin);
    s.push_str("\"\r\n");
    s.push_str("set \"DIR=%~dp0\"\r\n");
    s.push_str("if not exist \"%DIR%npb-gateway.exe\" (\r\n");
    s.push_str("  echo [错误] 本目录没有 npb-gateway.exe。请把本脚本放到老笔记本的“部署包目录”后再运行。\r\n");
    s.push_str("  pause\r\n  exit /b 1\r\n)\r\n");
    // 备份
    s.push_str("echo ===== [1/6] 备份现有文件（不改网络配置）=====\r\n");
    s.push_str("set \"TS=%DATE:~0,4%%DATE:~5,2%%DATE:~8,2%-%TIME:~0,2%%TIME:~3,2%%TIME:~6,2%\"\r\n");
    s.push_str("set \"TS=%TS: =0%\"\r\n");
    s.push_str("set \"BAK=%DIR%_backup_%TS%\"\r\n");
    s.push_str("mkdir \"%BAK%\" >nul 2>&1\r\n");
    s.push_str("xcopy \"%DIR%*.exe\" \"%BAK%\\\" /Y >nul 2>&1\r\n");
    s.push_str("xcopy \"%DIR%*.bat\" \"%BAK%\\\" /Y >nul 2>&1\r\n");
    s.push_str("xcopy \"%DIR%*.vbs\" \"%BAK%\\\" /Y >nul 2>&1\r\n");
    s.push_str("xcopy \"%DIR%*.ps1\" \"%BAK%\\\" /Y >nul 2>&1\r\n");
    s.push_str("xcopy \"%DIR%ui\" \"%BAK%\\ui\\\" /E /Y >nul 2>&1\r\n");
    s.push_str("echo   备份完成: %BAK%\r\n");
    // 下载
    s.push_str("echo ===== [2/6] 下载升级包 =====\r\n");
    s.push_str("curl.exe -s -f -o \"%TEMP%\\ck-upgrade.zip\" \"%ORIGIN%/upgrade-package.zip\" 2>nul\r\n");
    s.push_str("if not exist \"%TEMP%\\ck-upgrade.zip\" (\r\n");
    s.push_str("  powershell -NoProfile -Command \"(New-Object Net.WebClient).DownloadFile('%ORIGIN%/upgrade-package.zip','%TEMP%\\ck-upgrade.zip')\" 2>nul\r\n)\r\n");
    s.push_str("if not exist \"%TEMP%\\ck-upgrade.zip\" (\r\n");
    s.push_str("  echo [错误] 下载失败——请确认开发机网关在运行、且同一局域网（%ORIGIN%）。\r\n");
    s.push_str("  pause\r\n  exit /b 1\r\n)\r\n");
    s.push_str("echo   已下载到 %TEMP%\\ck-upgrade.zip\r\n");
    // 停止本应用
    s.push_str("echo ===== [3/6] 停止本应用（仅 npb-gateway / 看门狗；不动系统服务）=====\r\n");
    s.push_str("taskkill /F /IM npb-gateway.exe >nul 2>&1\r\n");
    s.push_str("taskkill /F /IM wscript.exe >nul 2>&1\r\n");
    s.push_str("ping -n 3 127.0.0.1 >nul\r\n");
    // 解压覆盖
    s.push_str("echo ===== [4/6] 解压覆盖（只覆盖本应用文件）=====\r\n");
    s.push_str("powershell -NoProfile -Command \"Expand-Archive -Force -LiteralPath '%TEMP%\\ck-upgrade.zip' -DestinationPath '%DIR%'\"\r\n");
    // 重启看门狗
    s.push_str("echo ===== [5/6] 重启看门狗 =====\r\n");
    s.push_str("if exist \"%DIR%watchdog.vbs\" ( start \"\" wscript.exe \"%DIR%watchdog.vbs\" )\r\n");
    s.push_str("ping -n 4 127.0.0.1 >nul\r\n");
    // 自检
    s.push_str("echo ===== [6/6] 自检（本机网关）=====\r\n");
    s.push_str("curl.exe -s -m 5 http://127.0.0.1:3000/v1/report.txt\r\n");
    s.push_str("echo.\r\n");
    s.push_str("curl.exe -s -m 5 -o nul -w \"页面 HTTP: %%{http_code}\\n\" http://127.0.0.1:3000/\r\n");
    s.push_str("echo.\r\n");
    s.push_str("echo 完成。**本脚本未修改任何网络配置**（IP / DNS / 代理 / hosts / 防火墙 / 路由）。\r\n");
    s.push_str("echo 若页面标题仍为旧名，请刷新浏览器（Ctrl+F5）；仍异常时可用 %BAK% 回滚。\r\n");
    s.push_str("pause\r\n");
    s
}

fn serve_ui(
    ui_dir: &Option<std::path::PathBuf>,
    target: &str,
    headers: &[String],
    port: u16,
) -> Option<Vec<u8>> {
    let dir = ui_dir.as_ref()?;
    let name = if target == "/" { "/index.html" } else { target };
    // ---- 动态脚本：按 Host 生成（不读磁盘，IP 漂移免疫） ----
    if name == "/run-probe.bat" {
        let origin = request_origin(headers, port);
        let body = run_probe_bat(&origin);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\n\
Content-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let mut out = header.into_bytes();
        out.extend(body);
        return Some(out);
    }
    let ctype = UI_ALLOW.iter().find(|(p, _)| *p == name)?.1;
    let path = dir.join(name.trim_start_matches('/'));
    // 防穿越兜底：只接受白名单文件名
    let fname = path.file_name()?.to_str()?;
    let allow = ["index.html", "manifest_ui.js", "manifest_ui_bg.wasm", "cloud-probe.exe", "cloud-discover.exe", "run-probe.bat",
        "net-check.bat", "net-repair.bat", "net-diagnose.ps1", "net-repair.ps1",
        "net-check-one.bat", "net-repair-one.bat", "compat-test.html",
        "upgrade-package.zip"].contains(&fname);
    if !allow {
        eprintln!("serve_ui: 白名单外拒绝 {name}");
        return None;
    }
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("serve_ui: 读取失败 {} ({e})", path.display());
            return None;
        }
    };
    // 注意：HTTP 响应头必须用 CRLF（v0.106 修正：原为裸 LF，浏览器容忍但严格客户端如
    // Node fetch/undici 会解析失败——本地预验因此暴露）。
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    let mut resp = header.into_bytes();
    resp.extend_from_slice(&bytes);
    Some(resp)
}