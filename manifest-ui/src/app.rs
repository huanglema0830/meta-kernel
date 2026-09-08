//! 应用层（DOM 绑定 + 状态；wasm 单线程 → thread_local 状态，跨 callback 共享）。

use npb_appkit::lifecycle::KernelEvent;
use npb_appkit::{LifecycleEngine, Namer, Speaker};
use std::cell::RefCell;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{console, Event, EventSource, HtmlButtonElement, HtmlDivElement, HtmlInputElement,
    HtmlTextAreaElement, MessageEvent};

const LS_ENTRIES: &str = "mj_entries_v1";
const LS_GW: &str = "mj_gw";

/// 显化条目（历史持久化：localStorage 行式 JSON-lite）。
#[derive(Clone, Debug)]
struct Entry {
    id: String,
    raw: String,
    seed: f32,
    created: String,
    lifecycle: u16,
    rounds: u32,
    early: u32,
}

impl Entry {
    fn to_line(&self) -> String {
        format!(
            "{{\"id\":\"{}\",\"seed\":{:.6},\"lifecycle\":{},\"rounds\":{},\"early\":{},\"created\":\"{}\",\"raw\":\"{}\"}}",
            self.id, self.seed, self.lifecycle, self.rounds, self.early,
            self.created, self.raw.replace('"', "\\\"")
        )
    }
    fn from_line(l: &str) -> Option<Entry> {
        fn f(s: &str, k: &str) -> Option<String> {
            let n = format!("\"{k}\"");
            let i = s.find(&n)?;
            let r = &s[i + n.len()..];
            let c = r.find(':')? + 1;
            let v: String = r[c..].trim_start().chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '"'
                    || c.is_alphabetic() || *c == ' ' || *c == '\\' || (*c as u32) > 127)
                .collect();
            Some(v.trim_matches('"').to_string())
        }
        Some(Entry {
            id: f(l, "id")?,
            raw: f(l, "raw")?.replace("\\\"", "\""),
            seed: f(l, "seed")?.parse().ok()?,
            created: f(l, "created")?,
            lifecycle: f(l, "lifecycle")?.parse().ok()?,
            rounds: f(l, "rounds")?.parse().ok()?,
            early: f(l, "early")?.parse().ok()?,
        })
    }
}

struct LogLine { lifecycle: u16, text: String }

thread_local! { static APP: RefCell<App> = RefCell::new(App::new()); }

struct App {
    gw: String,
    engine: LifecycleEngine,
    entries: Vec<Entry>,
    active: Option<usize>,
    log: Vec<LogLine>,
    es: Option<EventSource>,
}

fn doc() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}
fn el<T: JsCast>(id: &str) -> T {
    doc().get_element_by_id(id).unwrap().unchecked_into::<T>()
}
fn set_text(id: &str, t: &str) {
    if let Some(n) = doc().get_element_by_id(id) {
        n.set_text_content(Some(t));
    }
}
fn storage() -> Option<web_sys::Storage> {
    web_sys::window().unwrap().local_storage().ok().flatten()
}
fn status(txt: &str, ok: bool) {
    if let Some(n) = doc().get_element_by_id("status") {
        n.set_text_content(Some(txt));
        let cls = if ok { "status ok" } else { "status bad" };
        let _ = n.set_attribute("class", cls);
    }
}

impl App {
    fn new() -> Self {
        let gw = storage().and_then(|s| s.get_item(LS_GW).ok().flatten())
            .unwrap_or_else(|| "http://127.0.0.1:3000".into());
        let entries = storage().and_then(|s| s.get_item(LS_ENTRIES).ok().flatten())
            .map(|raw| raw.split('\n').filter_map(Entry::from_line).collect::<Vec<_>>())
            .unwrap_or_default();
        Self { gw, engine: LifecycleEngine::new(), entries, active: None, log: Vec::new(), es: None }
    }

    fn save_entries(&self) {
        if let Some(s) = storage() {
            let joined = self.entries.iter().map(|e| e.to_line()).collect::<Vec<_>>().join("\n");
            let _ = s.set_item(LS_ENTRIES, &joined);
            let _ = s.set_item(LS_GW, &self.gw);
        }
    }

    fn current(&self) -> Option<&Entry> { self.active.and_then(|i| self.entries.get(i)) }

    fn seed_of(text: &str) -> f32 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mix = ((h >> 32) as u32) ^ (h as u32);
        let x = f64::from(mix) / f64::from(u32::MAX);
        0.25 + 0.70 * x as f32
    }

    fn band_name(state: u16) -> String { Namer::band_of(state) }

    fn render(&self) {
        match self.current() {
            Some(e) => {
                set_text("raw", &e.raw);
                set_text("meta", &format!("{} · seed={:.4} · {}", e.id, e.seed, e.created));
                set_text("band", &Self::band_name(e.lifecycle));
                let mut rounds = format!("轮次 {}", e.rounds);
                if e.early > 0 { rounds.push_str(&format!(" · 早退 {}", e.early)); }
                set_text("rounds", &rounds);
                let life: HtmlDivElement = el("life");
                life.set_text_content(Some(&format!("{:02}", e.lifecycle)));
                let cls = if e.lifecycle == 0 { "life void" } else { "life lit" };
                let _ = life.set_attribute("class", cls);
                let pct = if e.lifecycle == 0 { 0.0 } else { (e.lifecycle as f32 / 99.0 * 100.0).min(100.0) };
                let _ = el::<HtmlDivElement>("gauge").set_attribute("style", &format!("width:{pct:.0}%"));
                let hint = if e.lifecycle == 99 { "极显圆满——可归档本轮，等待回融".to_string() }
                    else if e.lifecycle == 0 { "锚点态——点击「点亮」注入这条念头".to_string() }
                    else { format!("推进中 {}——可注入补充扰动或归档", Self::band_name(e.lifecycle)) };
                set_text("intent", &hint);
            }
            None => {
                set_text("raw", "");
                set_text("meta", "（无激活条目——点亮一条念头）");
                set_text("band", "锚点");
                set_text("rounds", "");
                let life: HtmlDivElement = el("life");
                life.set_text_content(Some("00"));
                let _ = life.set_attribute("class", "life void");
                let _ = el::<HtmlDivElement>("gauge").set_attribute("style", "width:0%");
                set_text("intent", "点击「点亮」注入一条念头");
            }
        }
        // 日志流（最近 8 条，倒序新在前）
        let ul = el::<web_sys::HtmlUListElement>("log");
        while let Some(c) = ul.first_child() { let _ = ul.remove_child(&c); }
        if self.log.is_empty() {
            let li = doc().create_element("li").unwrap();
            li.set_attribute("class", "dim").ok();
            li.set_text_content(Some("（暂无事件——点亮后此处出现显化日志）"));
            let _ = ul.append_child(&li);
        } else {
            for l in self.log.iter().rev().take(8) {
                let li = doc().create_element("li").unwrap();
                li.set_text_content(Some(&format!("[{}] {}", Self::band_name(l.lifecycle), l.text)));
                let _ = ul.append_child(&li);
            }
        }
        // 历史条目
        let ul2 = el::<web_sys::HtmlUListElement>("entries");
        while let Some(c) = ul2.first_child() { let _ = ul2.remove_child(&c); }
        for (i, e) in self.entries.iter().enumerate() {
            let li = doc().create_element("li").unwrap();
            let _ = li.set_attribute("class", "entry");
            let b = doc().create_element("span").unwrap();
            let _ = b.set_attribute("class", "e-badge");
            b.set_text_content(Some(&format!("{:02}", e.lifecycle)));
            let r = doc().create_element("span").unwrap();
            let _ = r.set_attribute("class", "e-raw");
            r.set_text_content(Some(&e.raw));
            let _ = li.append_child(&b);
            let _ = li.append_child(&r);
            let idx = i;
            let li_c = li.clone();
            let click = Closure::wrap(Box::new(move |_e: Event| {
                APP.with(|a| a.borrow_mut().activate(idx));
            }) as Box<dyn FnMut(Event)>);
            let _ = li_c.add_event_listener_with_callback("click", click.as_ref().unchecked_ref());
            click.forget();
            let _ = ul2.append_child(&li);
        }
    }

    fn activate(&mut self, idx: usize) {
        if idx >= self.entries.len() { return; }
        self.active = Some(idx);
        self.engine = LifecycleEngine::new();
        self.log.clear();
        let e = &self.entries[idx];
        self.engine.state = e.lifecycle;
        self.engine.rounds = e.rounds;
        self.render();
    }

    fn ingest(&mut self, kind: &str, data: &str) {
        if kind == "snapshot" { return; }
        let ev = npb_appkit::normalize::normalize(kind, data);
        let before = self.engine.state;
        let changed = self.engine.apply(ev);
        let mut text = None;
        if kind == "instruction" {
            if let Some(st) = Speaker::statement_of_instruction(data) {
                text = Some(format!("指令 · {}", st.text));
            }
        }
        if changed || text.is_some() {
            let t = text.unwrap_or_else(|| format!("{} → {}（{}）", before, self.engine.state,
                if self.engine.state == 0 { "回融".to_string() } else { Self::band_name(self.engine.state) }));
            self.log.push(LogLine { lifecycle: self.engine.state, text: t });
            if self.log.len() > 64 { self.log.truncate(64); }
            if let Some(i) = self.active {
                self.entries[i].lifecycle = self.engine.state;
                self.entries[i].rounds = self.engine.rounds;
            }
            self.save_entries();
            self.render();
        }
    }

    fn connect(&mut self) {
        let v = el::<HtmlInputElement>("gw").value().trim().to_string();
        if !v.is_empty() { self.gw = v; }
        el::<HtmlInputElement>("gw").set_value(&self.gw);
        if let Some(s) = storage() { let _ = s.set_item(LS_GW, &self.gw); }
        let url = format!("{}/v1/events", self.gw);
        match EventSource::new(&url) {
            Ok(es) => {
                bind_named(&es, "open", |_| status("已订阅 /v1/events", true));
                bind_named(&es, "state_change", |e| {
                    if let Some(d) = e.data().as_string() { APP.with(|a| a.borrow_mut().ingest("state_change", &d)); }
                });
                bind_named(&es, "instruction", |e| {
                    if let Some(d) = e.data().as_string() { APP.with(|a| a.borrow_mut().ingest("instruction", &d)); }
                });
                bind_named(&es, "snapshot", |_| {});
                self.es = Some(es);
                status("已订阅 /v1/events", true);
            }
            Err(e) => { console::error_1(&e); status("订阅失败（网关离线或 CORS）", false); }
        }
        self.render();
    }
}

/// 给 EventSource 绑定命名事件（EventSource 继承 EventTarget → 泛型监听）。
fn bind_named<F>(es: &EventSource, event_type: &str, mut f: F)
where F: FnMut(MessageEvent) + 'static {
    let et: &web_sys::EventTarget = es.unchecked_ref();
    let c = Closure::wrap(Box::new(move |e: Event| {
        let me = e.unchecked_into::<MessageEvent>();
        f(me);
    }) as Box<dyn FnMut(Event)>);
    let _ = et.add_event_listener_with_callback(event_type, c.as_ref().unchecked_ref());
    c.forget();
}

async fn post_push(gw: &str, seed: f32) -> Result<String, JsValue> {
    let url = format!("{gw}/v1/push");
    let body = format!("{{\"seed\":{seed:.4}}}");
    http_json("POST", &url, Some(&body)).await
}

/// 统一 HTTP：fetch → Promise(JsFuture) → Response → text。
async fn http_json(method: &str, url: &str, body: Option<&str>) -> Result<String, JsValue> {
    use wasm_bindgen_futures::JsFuture;
    let win = web_sys::window().unwrap();
    let mut init = web_sys::RequestInit::new();
    init.method(method);
    init.mode(web_sys::RequestMode::Cors);
    if let Some(b) = body {
        init.body(Some(&JsValue::from_str(b)));
    }
    let req = web_sys::Request::new_with_str_and_init(url, &init)?;
    if body.is_some() {
        req.headers().set("Content-Type", "application/json")?;
    }
    let promise = win.fetch_with_request(&req);
    let v = JsFuture::from(promise).await?;
    let resp: web_sys::Response = v.dyn_into()?;
    let txt = JsFuture::from(resp.text()?).await?;
    txt.as_string().ok_or_else(|| JsValue::from_str("not text"))
}

impl App {
    fn light(&mut self) {
        let raw = el::<HtmlTextAreaElement>("input").value().trim().to_string();
        if raw.is_empty() { return; }
        let seed = Self::seed_of(&raw);
        let id = format!("m-{:08x}", ((seed as f64 * 1e9) as u64) & 0xffff_ffff);
        self.entries.push(Entry {
            id, raw: raw.clone(), seed,
            created: chrono_lite(), lifecycle: 0, rounds: 0, early: 0,
        });
        self.active = Some(self.entries.len() - 1);
        self.engine = LifecycleEngine::new();
        self.log.clear();
        self.render();
        let gw = self.gw.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match post_push(&gw, seed).await {
                Ok(t) if t.contains("accepted") => {
                    APP.with(|a| { let mut x = a.borrow_mut();
                        x.log.push(LogLine { lifecycle: 0, text: format!("点亮：注入 seed={seed:.4}") });
                        x.save_entries();
                        x.render();
                    });
                    status("已点亮（accepted）", true);
                }
                Ok(t) => { console::warn_1(&JsValue::from_str(&t)); status("push 被拒", false); }
                Err(e) => { console::error_1(&e); status("push 失败（网关离线？）", false); }
            }
        });
    }

    fn boost(&mut self) {
        let Some(e) = self.current() else { return };
        let seed = (e.seed + 0.05).min(0.95);
        let gw = self.gw.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = post_push(&gw, seed).await;
        });
    }

    fn archive(&mut self) {
        let _ = self.engine.apply(KernelEvent::Reset);
        if let Some(i) = self.active {
            self.entries[i].lifecycle = self.engine.state;
            self.entries[i].rounds = self.engine.rounds;
            self.entries[i].early += 1;
        }
        self.log.push(LogLine { lifecycle: 0, text: "归档：早退回融 0 锚点".into() });
        self.save_entries();
        self.render();
    }
}

pub fn init() {
    APP.with(|app| {
        let mut a = app.borrow_mut();
        el::<HtmlInputElement>("gw").set_value(&a.gw);
        bind_click("btn-connect", move || APP.with(|x| x.borrow_mut().connect()));
        bind_click("btn-light", move || APP.with(|x| x.borrow_mut().light()));
        bind_click("btn-boost", move || APP.with(|x| x.borrow_mut().boost()));
        bind_click("btn-archive", move || APP.with(|x| x.borrow_mut().archive()));
        a.connect();
        a.render();
    });
}

fn bind_click(id: &str, f: impl Fn() + 'static) {
    let btn: HtmlButtonElement = el(id);
    let c = Closure::wrap(Box::new(move |_e: Event| f()) as Box<dyn FnMut(Event)>);
    let _ = btn.add_event_listener_with_callback("click", c.as_ref().unchecked_ref());
    c.forget();
}

/// 轻量时间戳（避免引入 chrono：零依赖保持）。
fn chrono_lite() -> String {
    js_sys::Date::new_0().to_locale_time_string("zh-CN").as_string().unwrap_or_default()
}
