//! # manifest-ui — Manifest Journal Web 前端（Trunk/wasm-bindgen CSR）
//!
//! 复用 npb-appkit 纯逻辑（`default-features=false`：LifecycleEngine / normalize /
//! Namer / Speaker），网络经浏览器 HTTP(fetch)/SSE(EventSource) 连 L3 网关
//! （wasm 无 std::net，EventPipe 属 net feature，前端不可用——这是设计约束，见 docs/）。
//! 历史条目存 localStorage（键 mj_entries_v1 / mj_gw / mj_active）。

mod app;

use wasm_bindgen::prelude::*;

/// 浏览器入口：挂载 DOM 事件并连接默认网关。
#[wasm_bindgen(start)]
pub fn run() {
    app::init();
    console_error_panic_hook_not_needed();
}

fn console_error_panic_hook_not_needed() {
    // 恐慌信息经 wasm-bindgen 默认打到 console.error；无需额外 hook（零依赖保持）。
}
