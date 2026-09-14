//! L6 · 正源浏览器应用层（l6_app）。
//!
//! 职责（对应设计 L6_DESIGN 的 l6_app / l6_render / l6_discover 分区）：
//! - 视图切换（诊断台默认 / 显化台 / 设备台 / 设置）；
//! - 诊断呈现：拉取网关最新采集 → 本地 L5 号脉（不重判·不加工）→ 四场/结论/成因/建议/溯源；
//! - 语言自动适配（l6_i18n）：正文取 summary.<lang>；
//! - 设备发现交互：本机网关地址与一键探针 URL 提示；
//! - 授权链（菩萨戒）：关键操作需明确授权；授权日志追加式记录、可撤销。

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

use crate::app::{chrono_lite, doc, el, http_json, set_text, storage};
use crate::l6_i18n as i18n;

/// APP 全局（从 app 模块借用）。
fn app_gw() -> String {
    let s = storage().and_then(|s| s.get_item("mj_gw").ok().flatten()).unwrap_or_default();
    if s.is_empty() {
        // 退化：直接读输入框
        el::<web_sys::HtmlInputElement>("gw").value()
    } else {
        s
    }
}

fn ls_get(key: &str) -> String {
    storage().and_then(|s| s.get_item(key).ok().flatten()).unwrap_or_default()
}
fn ls_set(key: &str, val: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, val);
    }
}

fn set_attr(id: &str, name: &str, val: &str) {
    let e: HtmlElement = el(id);
    let _ = e.set_attribute(name, val);
}

/// 视图切换（tabs）。固定 id 切换（零额外 web-sys 特性依赖）。
pub fn show(view_id: &str, tab_id: &str) {
    let views = ["view-diag", "view-manifest", "view-device", "view-settings"];
    let tabs = ["tab-diag", "tab-manifest", "tab-device", "tab-settings"];
    for v in views {
        set_attr(v, "class", if v == view_id { "view active" } else { "view" });
    }
    for t in tabs {
        set_attr(t, "class", if t == tab_id { "tab active" } else { "tab" });
    }
}

/// 语言初始化（状态带 + 设置页联动，变更即记录并刷新诊断）。
pub fn lang_init() {
    let cur = {
        let v = ls_get("l6_lang");
        if v.is_empty() { "auto".to_string() } else { v }
    };
    let _ = el::<web_sys::HtmlSelectElement>("lang").set_value(&cur);
    let _ = el::<web_sys::HtmlSelectElement>("lang2").set_value(&cur);
    for id in ["lang", "lang2"] {
        let node: HtmlElement = el(id);
        let id2 = id.to_string();
        let c = Closure::wrap(Box::new(move |_e: web_sys::Event| {
            lang_changed(&id2);
        }) as Box<dyn FnMut(web_sys::Event)>);
        let _ = node.add_event_listener_with_callback("change", c.as_ref().unchecked_ref());
        c.forget();
    }
}

fn lang_changed(which: &str) {
    let v = el::<web_sys::HtmlSelectElement>(which).value();
    ls_set("l6_lang", &v);
    if which == "lang" {
        let _ = el::<web_sys::HtmlSelectElement>("lang2").set_value(&v);
    } else {
        let _ = el::<web_sys::HtmlSelectElement>("lang").set_value(&v);
    }
    refresh_diag();
}

fn current_key() -> String {
    let nav = web_sys::window()
        .and_then(|w| w.navigator().language())
        .unwrap_or_else(|| "en".into());
    i18n::summary_key_for(&nav, &ls_get("l6_lang"))
}

fn is_zh() -> bool {
    web_sys::window()
        .and_then(|w| w.navigator().language())
        .map(|l| l.to_lowercase().starts_with("zh"))
        .unwrap_or(false)
}

/// 授权确认（授权链）：用户明确授权才执行；日志追加式（不可篡改；设置页可撤销）。
pub fn auth_confirm(op: &str) -> bool {
    let msg = format!("授权确认：\n{op}\n\n（允许后将记入授权日志，可随时在「设置」中撤销）");
    let ok = web_sys::window()
        .and_then(|w| w.confirm_with_message(&msg).ok())
        .unwrap_or(false);
    auth_append(op, ok);
    ok
}

fn auth_append(op: &str, allowed: bool) {
    let log = ls_get("l6_auth");
    let line = format!("{}|{}|{}", chrono_lite(), op, if allowed { "允许" } else { "拒绝" });
    let new = if log.is_empty() { line } else { format!("{log}\n{line}") };
    ls_set("l6_auth", &new);
    render_auth();
}

pub fn render_auth() {
    let log = ls_get("l6_auth");
    let lines: Vec<&str> = log.lines().filter(|l| !l.is_empty()).collect();
    set_text("auth-count", &format!("授权记录 {} 条（追加式，不可篡改；撤销即清空）", lines.len()));
    let ul: web_sys::HtmlUListElement = el("auth-log");
    ul.set_inner_html("");
    for l in lines.iter().rev().take(8) {
        if let Ok(li) = doc().create_element("li") {
            li.set_text_content(Some(l));
            let _ = ul.append_child(&li);
        }
    }
}

pub fn auth_clear() {
    ls_set("l6_auth", "");
    render_auth();
}

/// 设备台提示（本机网关 + 一键探针 URL）。
pub fn device_hint() {
    // 同源优先：origin 含协议，最准确（https 访问时不会错写成 http）；
    // 回退 host；再回退默认 127.0.0.1:3000（file:// 直开场景）。
    let base = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .filter(|o| !o.is_empty() && o != "null")
        .or_else(|| {
            web_sys::window()
                .and_then(|w| w.location().host().ok())
                .filter(|h| !h.is_empty())
                .map(|h| format!("http://{h}"))
        })
        .unwrap_or_else(|| "http://127.0.0.1:3000".into());
    set_text("dev-gw", &base);
    set_text("dev-url", &format!("{base}/run-probe.bat"));
    set_text("hint-url", &format!("{base}/run-probe.bat"));
    let a: HtmlElement = el("dev-probe");
    let _ = a.set_attribute("href", &format!("{base}/cloud-probe.exe"));
}

/// 解析 FieldReading JSON 的 s[7]。
fn parse_reading(json: &str) -> Option<[f64; 7]> {
    let key = "\"s\":[";
    let i = json.find(key)?;
    let rest = &json[i + key.len()..];
    let end = rest.find(']')?;
    let mut out = [0.0f64; 7];
    let mut n = 0;
    for part in rest[..end].split(',') {
        let v: f64 = part.trim().parse().ok()?;
        if n < 7 {
            out[n] = v;
            n += 1;
        }
    }
    if n == 7 { Some(out) } else { None }
}

/// 刷新诊断：GET /v1/probe → 本地 L5 号脉 → 渲染。
pub fn refresh_diag() {
    let gw = app_gw();
    let url = format!("{}/v1/probe", gw.trim_end_matches('/').to_string());
    wasm_bindgen_futures::spawn_local(async move {
        match http_json("GET", &url, None).await {
            Ok(t) => match parse_reading(&t) {
                Some(s) => render_diag(&s),
                None => show_empty(),
            },
            Err(_) => show_empty(),
        }
    });
}

fn show_empty() {
    set_attr("diag-empty", "style", "display:block");
    set_attr("diag-body", "style", "display:none");
    set_text("probe-time", "—");
}

/// 呈现（l6_render）：四场亢/枯/平 + 主结论 + 成因 + 建议 + 确信度 + 溯源 + 多语言正文。
fn render_diag(s: &[f64; 7]) {
    use meta_kernel_core::l5_baseline::BaselineField;
    use meta_kernel_core::l5_compare::{compare, Band};
    use meta_kernel_core::l5_diagnosis::{synthesize, Diagnosis, Traceability};
    use meta_kernel_core::l5_senses::decompose;
    use meta_kernel_core::l5_translate::{summarize_for, TERMS};

    let fields = decompose(s);
    let bl = BaselineField {
        earth: 1.0, water: 1.0, fire: 1.0, wind: 1.0,
        object: "probe-host", established: "health-heuristic",
    };
    let pattern = compare(&fields, &bl);
    let conclusion = synthesize(&fields, &bl, &pattern);
    let trace = Traceability {
        baseline_id: "b-health-heuristic".into(),
        object: "probe-host".into(),
        at: chrono_lite(),
        reproducible: true,
    };
    let d = Diagnosis {
        schema: 2,
        fields,
        pattern,
        conclusion: conclusion.clone(),
        trace: trace.clone(),
    };

    set_attr("diag-empty", "style", "display:none");
    set_attr("diag-body", "style", "display:block");

    let names = ["earth", "water", "fire", "wind"];
    let zh = is_zh();
    for (i, n) in names.iter().enumerate() {
        let band = pattern[i];
        let label = match band {
            Band::Kang => if zh { "亢" } else { "Kang" },
            Band::Ku => if zh { "枯" } else { "Ku" },
            Band::Ping => if zh { "平" } else { "Ping" },
        };
        set_text(&format!("v-{n}"), &format!("{}（{:.2}）", label, fields[i]));
        let cls = match band {
            Band::Kang => "band b-Kang",
            Band::Ku => "band b-Ku",
            Band::Ping => "band b-Ping",
        };
        set_attr(&format!("v-{n}"), "class", cls);
        let bar = match band {
            Band::Kang => "bar-kang",
            Band::Ku => "bar-ku",
            Band::Ping => "bar-ping",
        };
        set_attr(&format!("bar-{n}"), "class", bar);
    }

    // 语言自动适配：正文取 summary.<lang>；结论字段保留 L5 原文（不加工）
    let lang_key = current_key();
    let summary_text = summarize_for(TERMS, &lang_key, &d);
    let tone = i18n::tone_prefix(conclusion.confidence, zh);
    set_text("d-title", &format!("{tone}{}", conclusion.title));
    set_text("d-desc", &format!("{}（{}）", summary_text, i18n::lang_label(&lang_key)));
    set_text("d-cause", &format!("成因：{}", conclusion.cause));
    set_text("d-advice", &format!("建议：{}", conclusion.suggestion));
    let conf_pct = (conclusion.confidence * 100.0).round() as i64;
    set_text(
        "d-trace",
        &format!(
            "确信度 {}% ｜ 溯源：{} · {} · 可复现 {} ｜ 基线 {}",
            conf_pct, trace.object, trace.at, trace.reproducible, trace.baseline_id
        ),
    );
    set_text("probe-time", &chrono_lite());

    let n: u64 = ls_get("l6_diag_n").parse().unwrap_or(0) + 1;
    ls_set("l6_diag_n", &n.to_string());
    set_text(
        "grow-info",
        &format!(
            "诊断次数 {} ｜ 最近签名 [{:.3}, {:.3}, {:.3}, {:.3}, {:.3}, {:.3}, {:.3}]",
            n, s[0], s[1], s[2], s[3], s[4], s[5], s[6]
        ),
    );
}

/// 初始化绑定（由 app::init 调用）。
pub fn init_bindings() {
    lang_init();
    device_hint();
    render_auth();
    refresh_diag();
}
