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
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

fn serve_forever(listener: TcpListener, gw: Arc<Gateway>, stop: Arc<AtomicBool>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(s) => {
                let gw2 = Arc::clone(&gw);
                let stop2 = Arc::clone(&stop);
                std::thread::spawn(move || {
                    let _ = handle_conn(s, gw2, stop2);
                });
            }
            Err(_) => break,
        }
    }
}

/// 绑定 127.0.0.1:port（0 = 随机可用端口）并启动服务器线程。
pub fn spawn(port: u16) -> std::io::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let addr = listener.local_addr()?.to_string();
    let gw = Arc::new(Gateway::spawn());
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = Arc::clone(&stop);
    let gw2 = Arc::clone(&gw);
    let handle = std::thread::spawn(move || serve_forever(listener, gw2, s2));
    Ok(Server { addr, stop, handle: Some(handle) })
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

fn http_ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn http_err(code: u16, reason: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code,
        reason,
        body.len(),
        body
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

fn handle_conn(mut stream: TcpStream, gw: Arc<Gateway>, stop: Arc<AtomicBool>) -> std::io::Result<()> {
    let (request_line, headers, mut leftover) = read_request(&mut stream);
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 3 {
        return Ok(());
    }
    let method = parts[0];
    let target = parts[1];

    // ---- SSE 订阅（长连接；先推 snapshot 秒同步，后按边沿/指令推送） ----
    if method == "GET" && target == "/v1/events" {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n",
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

    let resp = match (method, target) {
        // 注入扰动：外部 push 驱动内核（网关不空转）
        ("POST", "/v1/push") => match parse_seed_body(&body) {
            Some(seed) => {
                if gw.push(seed) {
                    let t = gw.projection().t;
                    http_ok(&format!("{{\"accepted\":true,\"t\":{t},\"tag\":null}}"))
                } else {
                    http_ok("{\"accepted\":false,\"reason\":\"gate_rejected\",\"tag\":null}")
                }
            }
            None => http_err(400, "Bad Request", "{\"error\":\"seed_required\"}"),
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
