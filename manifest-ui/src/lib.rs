//! # manifest-ui — Manifest Journal Web 前端（Trunk/wasm-bindgen CSR）
//!
//! 复用 npb-appkit 纯逻辑（`default-features=false`：LifecycleEngine / normalize /
//! Namer / Speaker），网络经浏览器 HTTP(fetch)/SSE(EventSource) 连 L3 网关
//! （wasm 无 std::net，EventPipe 属 net feature，前端不可用——这是设计约束，见 docs/）。
//! 历史条目存 localStorage（键 mj_entries_v1 / mj_gw / mj_active）。

mod app;
mod l6_app;
mod l6_i18n;

use wasm_bindgen::prelude::*;

/// 浏览器入口：挂载 DOM 事件并连接默认网关。
#[wasm_bindgen(start)]
pub fn run() {
    app::init();
    console_error_panic_hook_not_needed();
}

// ===== v0.111 · 场域解析 / 映射 / 脸切换（内核逻辑 → 浏览器）=====
//
// 设计：**单一事实源**——公式与判定都在 `meta-kernel-core`，本处只是薄薄的 JSON 出入口；
// UI 侧不做任何重复计算（避免"两份公式各算各的"）。

use std::cell::RefCell;
use meta_kernel_core::gene_library::GeneLibrary;
use meta_kernel_core::l1_field_parse::{classify, parse, PageSignal};
use meta_kernel_core::l1_mapping::{render, seed_into, to_css, weights_of};
use meta_kernel_core::l6_face::{overlay, should_render, FaceMode};

thread_local! {
    /// 场域映射库（与基因库同源；启动时登记默认映射公式一次）。
    static FIELD_LIB: RefCell<GeneLibrary> = RefCell::new({
        let mut l = GeneLibrary::new();
        seed_into(&mut l);
        l
    });
}

/// **场域解析 + 映射 + 脸切换**（一步到位，输出纯 JSON 值）。
///
/// 输入 = 宿主抽取的页面信号（**内核不碰 DOM**）+ 脸模式键（`original`/`field`/`blend`）。
/// 输出 = 场域读数 / 内容类别 / 画面参数 / 叠层强度 / CSS 变量串 / 是否渲染叠层。
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn field_parse(
    text_len: u32,
    paragraph_count: u32,
    heading_count: u32,
    link_count: u32,
    image_count: u32,
    media_count: u32,
    interactive_count: u32,
    script_count: u32,
    mode: &str,
) -> String {
    let sig = PageSignal {
        text_len,
        paragraph_count,
        heading_count,
        link_count,
        image_count,
        media_count,
        interactive_count,
        script_count,
    };
    let reading = parse(&sig);
    let class = classify(&sig);
    let m = FaceMode::parse(mode);
    FIELD_LIB.with(|cell| {
        let lib = cell.borrow();
        let v = render(&reading, &lib);
        let css = to_css(&v);
        let o = overlay(m, &v, reading.confidence, css.clone());
        format!(
            "{{\"schema\":1,\"fields\":{{\"earth\":{:.6},\"water\":{:.6},\"fire\":{:.6},\"wind\":{:.6},\"confidence\":{:.6},\"dominant\":\"{}\"}},\"class\":{{\"code\":{},\"label\":\"{}\"}},\"visual\":{{\"hue\":{:.6},\"sat\":{:.6},\"light\":{:.6},\"contrast\":{:.6},\"tempo\":{:.6},\"density\":{:.6},\"radius\":{:.6}}},\"face\":{{\"mode\":\"{}\",\"label\":\"{}\",\"alpha\":{:.6},\"render\":{},\"css\":\"{}\"}},\"mapgeneweights_hue\":[{:.4},{:.4},{:.4},{:.4}]}}",
            reading.earth, reading.water, reading.fire, reading.wind, reading.confidence,
            reading.dominant_label(),
            class.code(), class.label(),
            v.hue, v.sat, v.light, v.contrast, v.tempo, v.density, v.radius,
            m.key(), m.label(), o.alpha, should_render(&o),
            css.replace('"', "'"),
            weights_of(&lib, 0)[0], weights_of(&lib, 0)[1], weights_of(&lib, 0)[2], weights_of(&lib, 0)[3],
        )
    })
}

fn console_error_panic_hook_not_needed() {
    // 恐慌信息经 wasm-bindgen 默认打到 console.error；无需额外 hook（零依赖保持）。
}
