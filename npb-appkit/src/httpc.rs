//! # 极简 HTTP 客户端基元（std；与 npb-gateway 服务器对称）
//!
//! 只实现 L3 协议所需：POST JSON（/v1/push、/v1/persist/*）与 GET SSE（/v1/events）。
//! 所有读操作带超时（默认 3s），杜绝无限阻塞；供 EventPipe 与 Manifest Journal 复用。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// 读取超时（SSE 长连接空闲仍会被网关 ping 保活；超时仅防悬挂）。
pub const IO_TIMEOUT: Duration = Duration::from_millis(3000);

fn connect(addr: &str) -> std::io::Result<TcpStream> {
    let s = TcpStream::connect(addr)?;
    s.set_read_timeout(Some(IO_TIMEOUT))?;
    Ok(s)
}

fn read_until_double_crlf(s: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        let n = s.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 64 * 1024 {
            break;
        }
    }
    Ok(buf)
}

/// POST JSON → 响应原始文本（含状态行与头；调用方自用子串断言）。
pub fn post_json(addr: &str, path: &str, body: &str) -> std::io::Result<String> {
    let mut s = connect(addr)?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())?;
    let mut out = String::new();
    let mut b = [0u8; 1024];
    loop {
        match s.read(&mut b) {
            Ok(0) => break,
            Ok(n) => out.push_str(&String::from_utf8_lossy(&b[..n])),
            Err(_) => break,
        }
    }
    Ok(out)
}

/// 建立 SSE 订阅：发送 GET /v1/events、消费响应头，返回就绪的数据流。
pub fn sse_open(addr: &str) -> std::io::Result<TcpStream> {
    let mut s = connect(addr)?;
    s.write_all(b"GET /v1/events HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")?;
    let head = read_until_double_crlf(&mut s)?;
    if !head.starts_with(b"HTTP/1.1 200") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("SSE open failed: {}", String::from_utf8_lossy(&head)),
        ));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_and_sse_roundtrip_against_live_gateway() {
        // dev-dependency：npb-gateway
        let srv = npb_gateway::http::spawn(0).expect("spawn gw");
        let addr = srv.addr.clone();
        // push 正
        let r = post_json(&addr, "/v1/push", r#"{"seed":0.5}"#).expect("post");
        assert!(r.contains("\"accepted\":true"), "{r}");
        // push 负拒
        let r2 = post_json(&addr, "/v1/push", r#"{"seed":-0.5}"#).expect("post");
        assert!(r2.contains("gate_rejected"), "{r2}");
        // health
        let h = post_json(&addr, "/v1/health", "").expect("health");
        assert!(h.contains("\"ok\":true"), "{h}");
        let mut srv2 = srv;
        srv2.stop();
    }

    #[test]
    fn sse_open_returns_ready_stream() {
        let srv = npb_gateway::http::spawn(0).expect("spawn gw");
        let addr = srv.addr.clone();
        let mut st = sse_open(&addr).expect("sse open");
        // 首帧应为 snapshot（200ms 内到达）
        let mut buf = [0u8; 4096];
        let n = st.read(&mut buf).unwrap_or(0);
        let got = String::from_utf8_lossy(&buf[..n]);
        assert!(got.contains("event: snapshot"), "{got}");
        let mut srv2 = srv;
        srv2.stop();
    }
}
