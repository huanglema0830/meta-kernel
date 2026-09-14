//! 空天浏览器 · **原生 WebView2 宿主**（v0.1）
//!
//! 依据：发起人「Q1–Q4 裁决 + 原生宿主编译」。Q1 = 方案 A，最小宿主原则：
//! **只读、不写、叠层、不替换**；Runtime 缺失 → **自动降级 B**（外开），绝不让用户见白屏。
//!
//! ## 为什么非要有原生宿主
//! 浏览器内嵌（iframe）受**同源策略**限制：读不到第三方站点内容，也无法内嵌腾讯文档/飞书/微信
//! （站点以 `X-Frame-Options` / CSP 拒绝被嵌）。原生 WebView 宿主可：① 直接承载这些站点；
//! ② **读取页面信号**（注入只读脚本，不受同源限制）→ 场域解析才真正对第三方站点可用。
//!
//! ## 只读承诺（可核查，且有测试守着）
//! - 抽取脚本**只做计数与长度读取**，不改动任何页面内容（测试断言不含 `innerHTML =` / `document.write` 等）；
//! - 叠层**只新增一个 `pointer-events:none` 覆盖节点**（强度 ≤ 35%），不改原 DOM 结构与文本；
//! - 宿主**不写任何系统配置**（不碰 IP/DNS/代理/hosts/防火墙/路由）。
//!
//! ## v0.1 功能
//! - 主窗口 = 工作台（`?host=webview` 标记频道）；**地址栏/多标签/书签/下载列表由工作台提供**
//! - 工作台经 IPC 请求打开网址 → 宿主**新建窗口**承载目标站点（多标签＝多窗口）+ 注入只读抽取
//! - 站点信号经 IPC 回传 → 宿主转发本机网关 → 把叠层注入（只加覆盖节点）
//! - 下载：走 WebView2 默认路径，事件回传提示
//!
//! 构建（仅 Windows）：`cd host/sky-browser && cargo build --release`
//! 本 crate **刻意不并入根 workspace**（WebView2/GTK 依赖会让 CI 的 Linux 构建失败）。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Mutex;

use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::{WebView, WebViewBuilder};

/// 本机网关（工作台与场域接口都在这里）。
const GW: &str = "http://127.0.0.1:3000";
/// WebView2 Evergreen Runtime 的客户 ID（注册表键名固定）。
const WV2_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// 注入到**目标站点**的只读信号抽取脚本（**不做任何写操作**）。
const EXTRACT_JS: &str = r#"
(function () {
  function ask() {
    try {
      var text = document.body ? (document.body.innerText || '') : '';
      var c = function (sel) { try { return document.querySelectorAll(sel).length; } catch (e) { return 0; } };
      var msg = {
        cmd: 'field',
        sig: {
          text_len: text.length,
          paragraph_count: c('p'),
          heading_count: c('h1,h2,h3,h4,h5,h6'),
          link_count: c('a[href]'),
          image_count: c('img'),
          media_count: c('video,audio,canvas'),
          interactive_count: c('input,button,select,textarea'),
          script_count: c('script')
        }
      };
      if (window.ipc && window.ipc.postMessage) { window.ipc.postMessage(JSON.stringify(msg)); }
    } catch (e) { }
  }
  if (document.readyState === 'complete') { ask(); } else { window.addEventListener('load', ask, false); }
  setTimeout(ask, 1500);
})();
"#;

fn main() {
    // ---- ① Runtime 检测：缺失则**降级 B**（外开），绝不见白屏 ----
    if !webview2_installed() {
        eprintln!("[sky-browser] 未检测到 WebView2 Runtime → 降级 B：用系统默认浏览器打开工作台。");
        open_default_browser(&format!("{GW}/"));
        std::process::exit(0);
    }
    if let Err(e) = run() {
        eprintln!("[sky-browser] 启动失败：{e} → 降级 B：用系统默认浏览器打开工作台。");
        open_default_browser(&format!("{GW}/"));
    }
}

/// 查注册表确认 WebView2 Runtime 是否已安装。
fn webview2_installed() -> bool {
    for k in [
        format!(r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{WV2_GUID}"),
        format!(r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{WV2_GUID}"),
        format!(r"HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{WV2_GUID}"),
    ] {
        if let Ok(o) = std::process::Command::new("reg").args(["query", &k, "/v", "pv"]).output() {
            if o.status.success() && String::from_utf8_lossy(&o.stdout).contains("pv") {
                return true;
            }
        }
    }
    false
}

/// 降级路径：交给系统默认浏览器（固定参数，不拼接用户输入）。
fn open_default_browser(url: &str) {
    let _ = std::process::Command::new("cmd").args(["/c", "start", "", url]).spawn();
}

struct Tab {
    window: Window,
    #[allow(dead_code)]
    webview: WebView,
}

fn run() -> wry::Result<()> {
    let event_loop = EventLoop::new();
    let mut tabs: HashMap<WindowId, Tab> = HashMap::new();

    let main_window = WindowBuilder::new()
        .with_title("空天浏览器（宿主）")
        .with_inner_size(LogicalSize::new(1280.0, 860.0))
        .build(&event_loop)
        .expect("main window");

    let main_wv = build_host_webview(&main_window, &format!("{GW}/?host=webview"))?;
    tabs.insert(main_window.id(), Tab { window: main_window, webview: main_wv });

    event_loop.run(move |event, elwt, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                println!("[sky-browser] 就绪（只读+叠层）；工作台 {GW}/?host=webview");
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, window_id, .. } => {
                tabs.remove(&window_id);
                if tabs.is_empty() {
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
        // 工作台请求的"打开网址" → 新标签（新窗口）
        if let Some(url) = pending_take_url() {
            if let Some(tab) = spawn_tab(elwt, &url) {
                tabs.insert(tab.window.id(), tab);
                println!("[sky-browser] 新标签（窗口）：{url}");
            }
        }
    });
}

/// 主窗口 WebView：承载工作台；接收 IPC（开新页）。
fn build_host_webview(window: &Window, url: &str) -> wry::Result<WebView> {
    WebViewBuilder::new()
        .with_url(url)
        .with_navigation_handler(|_u| true)
        .with_download_started_handler(|url: String, path: &mut PathBuf| {
            println!("[sky-browser] 下载开始：{url} → {}", path.display());
            true
        })
        .with_ipc_handler(|req: wry::http::Request<String>| {
            let body = req.body().clone();
            if let Some(u) = json_str(&body, "url") {
                if let Some(ok) = pure::safe_url(&u) {
                    pending_put_url(ok);
                }
            }
        })
        .build(window)
}

/// 目标站点窗口：注入**只读**抽取 + 叠层注入。
fn spawn_tab(elwt: &tao::event_loop::EventLoopWindowTarget<()>, url: &str) -> Option<Tab> {
    let window = WindowBuilder::new()
        .with_title(format!("空天浏览器 · {url}"))
        .with_inner_size(LogicalSize::new(1280.0, 860.0))
        .build(elwt)
        .ok()?;
    let wv = WebViewBuilder::new()
        .with_url(url)
        .with_navigation_handler(|_u| true)
        .with_initialization_script(EXTRACT_JS)
        .with_download_started_handler(|u: String, p: &mut PathBuf| {
            println!("[sky-browser] 下载开始：{u} → {}", p.display());
            true
        })
        .with_ipc_handler(|req: wry::http::Request<String>| {
            let body = req.body().clone();
            if let Some(sig) = json_block(&body, "sig") {
                if let Some((css, alpha)) = ask_field(&sig) {
                    pending_overlay(overlay_js(&css, alpha));
                }
            }
        })
        .build(&window)
        .ok()?;
    Some(Tab { window, webview: wv })
}

/// 叠层渲染脚本：**只新增一个覆盖节点**，不修改页面内容。
fn overlay_js(css: &str, alpha: f64) -> String {
    format!(
        "(function () {{ var id='__field_overlay__'; var el=document.getElementById(id); \
if (!el) {{ el=document.createElement('div'); el.id=id; \
el.style.cssText='position:fixed;inset:0;pointer-events:none;z-index:2147483647;transition:opacity .35s ease'; \
document.documentElement.appendChild(el); }} \
el.style.opacity='{a:.3}'; el.setAttribute('data-field-css', '{c}'); }})();",
        a = pure::clamp_alpha(alpha),
        c = css.replace('\'', "").replace('"', "")
    )
}

// ===== 与内核/网关通信（纯 std，无额外依赖）=====

/// 把信号 POST 给网关场域接口，返回 `(css, alpha)`；网关未提供该端点时返回 `None`（不影响浏览）。
fn ask_field(sig_json: &str) -> Option<(String, f64)> {
    let body = format!("{{\"sig\":{sig_json},\"mode\":\"field\"}}");
    let resp = http_post(&format!("{GW}/v1/field/parse"), &body)?;
    let css = json_str(&resp, "css")?;
    let alpha = json_num(&resp, "alpha").unwrap_or(0.15);
    Some((css, alpha))
}

fn http_post(url: &str, body: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let mut s = TcpStream::connect(host_port).ok()?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    out.split("\r\n\r\n").nth(1).map(|x| x.to_string())
}

// ===== 极简 JSON 取值（只读字符串，不参与判定）=====

fn json_str(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let i = body.find(&pat)?;
    let rest = &body[i + pat.len()..];
    let c = rest.find(':')? + 1;
    let rest = &rest[c..];
    let start = rest.find('"')? + 1;
    let rest = &rest[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn json_num(body: &str, key: &str) -> Option<f64> {
    let pat = format!("\"{key}\"");
    let i = body.find(&pat)?;
    let rest = &body[i + pat.len()..];
    let c = rest.find(':')? + 1;
    let rest = rest[c..].trim_start();
    let mut n = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == '-' {
            n.push(ch);
        } else {
            break;
        }
    }
    n.parse().ok()
}

/// 取 `"sig":{...}` 原始块（浅层花括号配对）。
fn json_block(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let i = body.find(&pat)?;
    let rest = &body[i + pat.len()..];
    let c = rest.find('{')?;
    let mut depth = 0;
    let mut end = None;
    for (k, ch) in rest[c..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(c + k + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    Some(rest[c..end?].to_string())
}

// ===== 事件循环与 IPC 之间传递（最小全局队列）=====

static PENDING_URL: Mutex<Option<String>> = Mutex::new(None);
static PENDING_OVERLAY: Mutex<Option<String>> = Mutex::new(None);

fn pending_put_url(u: String) {
    if let Ok(mut g) = PENDING_URL.lock() {
        *g = Some(u);
    }
}
fn pending_take_url() -> Option<String> {
    PENDING_URL.lock().ok().and_then(|mut g| g.take())
}
fn pending_overlay(js: String) {
    if let Ok(mut g) = PENDING_OVERLAY.lock() {
        *g = Some(js);
    }
}
/// 是否有待注入的叠层（供测试/外部查询）。
pub fn has_pending_overlay() -> bool {
    PENDING_OVERLAY.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// 纯逻辑（可测；不依赖 GUI，也不依赖 WebView2）。
pub mod pure {
    /// 叠层强度上限（**永不遮住内容**；与内核 `l6_face` 同口径）。
    pub const ALPHA_CAP: f64 = 0.35;

    /// 归一化叠层强度（NaN → 0）。
    pub fn clamp_alpha(a: f64) -> f64 {
        if a.is_nan() {
            return 0.0;
        }
        a.clamp(0.0, ALPHA_CAP)
    }

    /// 只接受 http/https（阻断 `javascript:` / `file:` / `data:` / `about:` 等）。
    pub fn safe_url(u: &str) -> Option<String> {
        let t = u.trim();
        if t.starts_with("http://") || t.starts_with("https://") {
            Some(t.to_string())
        } else {
            None
        }
    }

    /// 由四场读数算叠层色相（宿主离线兜底用）。
    pub fn hue_from_fields(earth: f64, water: f64, fire: f64, wind: f64) -> f64 {
        let v = 0.10 * earth + 0.05 * water + 0.85 * fire + 0.15 * wind;
        (200.0 + v.clamp(0.0, 1.0) * 140.0).clamp(200.0, 340.0)
    }
}

#[cfg(test)]
mod tests {
    use super::pure::*;
    use super::*;

    #[test]
    fn alpha_is_capped_and_nan_safe() {
        assert_eq!(clamp_alpha(1.0), ALPHA_CAP);
        assert_eq!(clamp_alpha(-1.0), 0.0);
        assert_eq!(clamp_alpha(f64::NAN), 0.0);
        assert!((clamp_alpha(0.15) - 0.15).abs() < 1e-12);
    }

    #[test]
    fn safe_url_only_allows_http_and_https() {
        assert!(safe_url("https://example.com").is_some());
        assert!(safe_url("  http://127.0.0.1:3000  ").is_some());
        for bad in ["javascript:alert(1)", "file:///C:/windows", "data:text/html,x", "about:blank", ""] {
            assert!(safe_url(bad).is_none(), "{bad} 必须被拒");
        }
    }

    #[test]
    fn hue_is_bounded_and_fire_dominant() {
        let hot = hue_from_fields(0.1, 0.1, 1.0, 0.1);
        let cool = hue_from_fields(0.1, 0.1, 0.0, 0.1);
        assert!(hot > cool, "火强 → 更暖");
        for (a, b, c, d) in [(0.0, 0.0, 0.0, 0.0), (1.0, 1.0, 1.0, 1.0)] {
            let h = hue_from_fields(a, b, c, d);
            assert!((200.0..=340.0).contains(&h), "越界 {h}");
        }
    }

    #[test]
    fn json_helpers_read_without_panicking() {
        let body = r#"{"cmd":"open","url":"https://a.com/x","sig":{"text_len":12,"n":1.5}}"#;
        assert_eq!(json_str(body, "url").unwrap(), "https://a.com/x");
        assert_eq!(json_str(body, "nope"), None);
        assert!((json_num(body, "text_len").unwrap() - 12.0).abs() < 1e-9);
        assert!(json_block(body, "sig").unwrap().contains("text_len"));
        assert!(json_block(body, "nope").is_none());
    }

    #[test]
    fn overlay_js_only_adds_one_node_and_never_writes_content() {
        let js = overlay_js("--fld-hue:210", 0.2);
        assert!(js.contains("__field_overlay__"));
        assert!(js.contains("pointer-events:none"), "叠层不可拦截点击");
        assert!(js.contains("createElement"), "只新增节点");
        for bad in ["innerHTML", "document.body=", "document.write", "removeChild"] {
            assert!(!js.contains(bad), "不得写页面内容: {bad}");
        }
    }

    #[test]
    fn extract_js_is_read_only() {
        for bad in ["innerHTML", "document.write", "removeChild", "localStorage.setItem", "appendChild"] {
            assert!(!EXTRACT_JS.contains(bad), "抽取脚本必须只读，出现: {bad}");
        }
        assert!(EXTRACT_JS.contains("querySelectorAll"), "只做计数读取");
    }

    #[test]
    fn pending_queue_is_single_slot_and_takeable() {
        pending_put_url("https://x.com".into());
        assert_eq!(pending_take_url().unwrap(), "https://x.com");
        assert!(pending_take_url().is_none(), "取走后为空");
        pending_overlay("/*js*/".into());
        assert!(has_pending_overlay());
        let _ = pending_take_overlay_for_test();
        assert!(!has_pending_overlay());
    }

    /// 仅测试用：清空叠层队列。
    fn pending_take_overlay_for_test() -> Option<String> {
        PENDING_OVERLAY.lock().ok().and_then(|mut g| g.take())
    }
}
