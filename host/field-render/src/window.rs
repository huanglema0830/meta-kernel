//! 场域呈现器 · **窗口宿主（winit 0.30 + wgpu surface + egui 0.30 UI）**
//!
//! 第三阶段·第二步（路线 2：`egui 0.30` 配 `wgpu 23`，**不改**已验证的场域管线）。
//!
//! ## 组成
//! - **场域 pass**：`preprocess.wgsl` → 双调排序 → `render.wgsl`（`record_field_passes`）
//! - **UI pass**：egui 0.30（`egui-winit` 收事件 + `egui-wgpu` 渲染），**同一个 encoder 内叠在场域之上**
//! - 不依赖 WebView2：只用 `winit` + `wgpu` + `egui` + 内核（内核自身零依赖）
//! - **无 `unsafe` 取巧**：`Instance::create_surface` 在 wgpu 23 是安全函数；
//!   `RenderPass::forget_lifetime()`（egui-wgpu 要求 `RenderPass<'static>`）同样是**安全函数**。
//!
//! ## 用法
//! - `field-render`：开窗；地址栏 / 多标签 / 四元组滑杆 / 下载 全部可用
//! - `field-render --frames N`：渲染 N 帧后自动测帧率 + 回读上屏像素判定，然后退出
//! - `field-render --sample N`：启动即选场域样本；`--quad t,c,l,s`：启动即设四元组
//! - `field-render --ui-selftest`：自动验收「多标签切换 + 下载写盘」

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use meta_kernel_core::gene_library::GeneLibrary;
use meta_kernel_core::l1_field_parse::FieldReading;
use meta_kernel_core::l1_mapping::{
    coherence_of, field_to_gabor_with, modulate_gabor, seed_coherence_into, seed_gabor_into,
};
use meta_kernel_core::l1_source_parse::parse_source;
use meta_kernel_core::l3_world::WorldModel;
use meta_kernel_core::l5_quad::{decide_probe, diagnose, regress, ProbeMode, ProbeReason, Quad};

use crate::{
    blend_with_world, build_pipeline, clear_color_of, elements_of_co, field_buffer_bytes,
    lod_n_for, record_field_passes, sample_pages, upload_co, Pipeline,
};

/// 启动选项。
pub struct RunOptions {
    /// `Some(n)`：渲染 n 帧后自动验收并退出；`None`：持续渲染直到用户关窗。
    pub frames: Option<u32>,
    /// 启动即选中的场域样本（0=纯文本页 1=纯图片页 2=纯图片页·紧张高）。
    pub sample: usize,
    /// 启动即设定的四元组 `[紧张, 平静, 喜欢, 安全]`。
    pub quad: Option<[f64; 4]>,
    /// 启动即加载的网址（`http`/`https`）。
    pub url: Option<String>,
    /// 无人值守验收：多标签切换 + 下载写盘。
    pub ui_selftest: bool,
}

pub fn run(opt: RunOptions) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("事件循环创建失败：{e}"))?;
    let mut app = App { opt, state: None, exit_code: 0 };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("事件循环运行失败：{e}"))?;
    if app.exit_code != 0 {
        std::process::exit(app.exit_code);
    }
    Ok(())
}

// ===================== 标签页 =====================

/// 一个标签页＝一个「场域页面」：源码 → 四场 → 画面。
#[derive(Clone)]
struct Tab {
    title: String,
    url: String,
    source: String,
    text: String,
    field: FieldReading,
    /// 该标签自己的四元组（切标签即切内在状态）。
    quad: Quad,
}

fn make_tab(name: &str, url: &str, html: &str, quad: Quad, lib: &GeneLibrary) -> Tab {
    let field = parse_source(html, "");
    // 链路自检：确保"该页面的场域 → Gabor"能算出来（值本身由 State::apply_active 现用现算）
    let _ = modulate_gabor(field_to_gabor_with(&field, lib), &quad);
    Tab {
        title: name.to_string(),
        url: url.to_string(),
        source: html.to_string(),
        text: text_digest(html, 4000),
        field,
        quad,
    }
}

/// 从 HTML 源码抽**纯文本摘要**（**仅用于侧栏展示**，不参与场域计算——场域走内核源码直解）。
fn text_digest(html: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let b = html.as_bytes();
    let mut i = 0usize;
    let mut in_tag = false;
    let mut skip = false; // 处于 script/style 内部
    while i < b.len() {
        let c = b[i];
        if c == b'<' {
            let head: String = html[i..].chars().take(9).collect::<String>().to_lowercase();
            if head.starts_with("<script") || head.starts_with("<style") {
                skip = true;
            } else if head.starts_with("</script") || head.starts_with("</style") {
                skip = false;
            }
            in_tag = true;
        } else if c == b'>' {
            in_tag = false;
        } else if !in_tag && !skip {
            let ch = html[i..].chars().next().unwrap_or(' ');
            out.push(if ch.is_whitespace() { ' ' } else { ch });
            i += ch.len_utf8();
            continue;
        }
        i += 1;
    }
    let squashed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if squashed.chars().count() > max_chars {
        squashed.chars().take(max_chars).collect::<String>() + " …"
    } else {
        squashed
    }
}

// ===================== 网址抓取（系统 curl，无 shell 拼接）=====================

const USER_AGENT: &str = "MetaKernel-SkyBrowser/0.1 (field-render; local)";
const MAX_SOURCE_BYTES: usize = 3 * 1024 * 1024;

/// 只放行 http/https —— 与内核侧同一条纪律，伪协议一律不当作网址。
fn safe_url(u: &str) -> Option<String> {
    let s = u.trim();
    if let Some(rest) = s.strip_prefix("http://").or_else(|| s.strip_prefix("https://")) {
        if !rest.is_empty() && !rest.starts_with('/') {
            return Some(s.to_string());
        }
    }
    None
}

/// 取网页源码：调用**系统 curl**（Windows 10+ 自带）。
/// `Command::new("curl").arg(...)` **逐参数传参**——不经 shell、无字符串拼接，URL 不会被当作命令解释。
fn fetch_source(url: &str) -> Result<String, String> {
    let out = std::process::Command::new("curl")
        .args(["-sSL", "--compressed", "--max-time", "15", "-A", USER_AGENT, url])
        .output()
        .map_err(|e| format!("调用系统 curl 失败：{e}（老笔记本无外网时同样会失败，属预期）"))?;
    if !out.status.success() {
        return Err(format!("curl 退出码 {:?}（取源码失败）", out.status.code()));
    }
    if out.stdout.is_empty() {
        return Err("返回内容为空".to_string());
    }
    let mut v = out.stdout;
    if v.len() > MAX_SOURCE_BYTES {
        v.truncate(MAX_SOURCE_BYTES);
    }
    Ok(String::from_utf8_lossy(&v).to_string())
}

// ===================== 应用（winit 0.30 ApplicationHandler）=====================

struct App {
    opt: RunOptions,
    state: Option<State>,
    exit_code: i32,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return; // 冗余 resumed（部分平台会连发）
        }
        match State::new(event_loop, &self.opt) {
            Ok(s) => self.state = Some(s),
            Err(e) => {
                eprintln!("[window] ✗ 初始化失败：{e}");
                self.exit_code = 2;
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else { return };
        if state.window.id() != id {
            return;
        }
        // 先交给 egui：被它消费的事件（例如在地址栏里打字）不再触发宿主快捷键
        let resp = state.egui_state.on_window_event(&state.window, &event);
        if resp.repaint {
            state.window.request_redraw();
        }
        if resp.consumed {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Character(c) => match c.as_str() {
                        "q" | "Q" => event_loop.exit(),
                        "1" => state.select_sample(0),
                        "2" => state.select_sample(1),
                        "3" => state.select_sample(2),
                        _ => {}
                    },
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                state.frame();
                if state.should_finish() {
                    let code = state.finish();
                    self.exit_code = code;
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

// ===================== UI 动作（先收集、再统一执行，避免借用冲突）=====================

#[derive(Clone)]
enum Action {
    Open(String),
    Refresh,
    NewTab,
    CloseTab(usize),
    SelectTab(usize),
    LoadSample(usize),
    Download,
    SetQuad(Quad),
}

// ===================== 渲染状态 =====================

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: Pipeline,
    lib: GeneLibrary,
    clear: wgpu::Color,

    // egui
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,

    // 浏览器状态
    tabs: Vec<Tab>,
    active: usize,
    addr: String,
    status: String,
    baseline: Quad,
    /// **世界模型**：承载场域的累积状态；画面呈现的是它的状态（与现实场合成）。
    world: WorldModel,
    /// 当前相位一致性（由「喜欢＝预测误差降低」得出）。
    coherence: f64,
    /// 当前渲染精度档位（动态 LOD 的活跃 splat 数）。
    n_elements: u32,
    natural_return: bool,
    probe_mode: ProbeMode,
    probe_reason: Option<ProbeReason>,
    deviation: f64,
    dominant: String,
    last_source_at: Option<Instant>,
    last_active_probe: Option<Instant>,

    // 后台抓取
    fetch_rx: Option<std::sync::mpsc::Receiver<Result<(String, String), String>>>,

    // 回读（客观验收 / 下载取帧）
    readback: wgpu::Buffer,
    bytes_per_row: u32,
    /// surface 纹理是否支持 `COPY_SRC`（支持则回读的是**真实上屏的那张纹理**）
    surface_copy_src: bool,
    /// 离屏目标
    offscreen: wgpu::Texture,
    offscreen_view: wgpu::TextureView,

    // 验收计数
    frames_target: Option<u32>,
    frames: u32,
    t_first: Option<Instant>,
    ui_selftest: bool,
    ui_selftest_done: bool,
}

impl State {
    fn new(event_loop: &ActiveEventLoop, opt: &RunOptions) -> Result<Self, String> {
        let attrs = Window::default_attributes()
            .with_title("空天浏览器 · 场域呈现（wgpu 直渲 + egui，不依赖 WebView2）")
            .with_inner_size(winit::dpi::LogicalSize::new(1080.0, 760.0));
        let window = Arc::new(event_loop.create_window(attrs).map_err(|e| format!("建窗失败：{e}"))?);
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        // 安全函数（wgpu 23）——不需要 unsafe，也不是 transmute 类取巧
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("创建 surface 失败：{e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "枚举不到可呈现的 GPU 适配器".to_string())?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("field-render-window"),
                required_features: wgpu::Features::empty(),
                // 关键：`downlevel_defaults()` 的 max_texture_dimension_2d=2048，
                // HiDPI 窗口可达 2134+ → `Surface::configure` 直接 panic。用 using_resolution 抬到适配器能力。
                required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| format!("创建设备失败：{e}"))?;

        let caps = surface.get_capabilities(&adapter);
        if caps.formats.is_empty() {
            return Err("surface 与适配器不兼容（无支持格式）".into());
        }
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
            wgpu::PresentMode::Fifo
        } else {
            caps.present_modes[0]
        };
        let surface_copy_src = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if surface_copy_src {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        let max_dim = device.limits().max_texture_dimension_2d;
        let (w, h) = (w.min(max_dim).max(1), h.min(max_dim).max(1));
        let config = wgpu::SurfaceConfiguration {
            usage,
            format,
            width: w,
            height: h,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        println!(
            "[window] 窗口 {}x{} · surface {:?} · {:?} · COPY_SRC={} · egui 0.30 + wgpu 23（无 WebView2）",
            w, h, format, present_mode, surface_copy_src
        );
        println!("[window] 适配器: {} / {:?} / {:?}", info.name, info.backend, info.device_type);

        let pipeline = build_pipeline(&device, crate::N_ELEMENTS, format);
        let mut lib = GeneLibrary::new();
        seed_gabor_into(&mut lib);
        seed_coherence_into(&mut lib); // 「喜欢＝预测误差降低」的系数（改库即改映射）

        // egui 三件套
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            None,
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

        let (readback, bytes_per_row, offscreen, offscreen_view) = make_targets(&device, format, w, h);

        let baseline = Quad { tension: 0.2, calm: 0.6, liking: 0.5, safety: 0.6 };
        let (text_html, image_html) = sample_pages();
        let hot = Quad { tension: 1.0, calm: 0.1, liking: 0.4, safety: 0.3 };
        let mut tabs = vec![
            make_tab("纯文本页", "sample:text", &text_html, baseline, &lib),
            make_tab("纯图片页", "sample:image", &image_html, baseline, &lib),
            make_tab("纯图片页·紧张高", "sample:image-hot", &image_html, hot, &lib),
        ];
        if let Some(q) = opt.quad {
            let q = Quad { tension: q[0], calm: q[1], liking: q[2], safety: q[3] };
            for t in tabs.iter_mut() {
                t.quad = q;
            }
        }
        let initial = opt.sample.min(tabs.len() - 1);

        let mut st = State {
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            lib,
            clear: wgpu::Color::BLACK,
            egui_ctx,
            egui_state,
            egui_renderer,
            tabs,
            active: initial,
            addr: opt.url.clone().unwrap_or_else(|| "sample:text".to_string()),
            status: "就绪。地址栏输入 http/https 网址后回车即可取源码 → 场域呈现。".to_string(),
            baseline,
            world: WorldModel::new(),
            coherence: 0.0,
            n_elements: crate::N_ELEMENTS,
            natural_return: false,
            probe_mode: ProbeMode::Passive,
            probe_reason: None,
            deviation: 0.0,
            dominant: "—".to_string(),
            last_source_at: Some(Instant::now()),
            last_active_probe: None,
            fetch_rx: None,
            readback,
            bytes_per_row,
            surface_copy_src,
            offscreen,
            offscreen_view,
            frames_target: opt.frames,
            frames: 0,
            t_first: None,
            ui_selftest: opt.ui_selftest,
            ui_selftest_done: false,
        };
        st.apply_active();
        if let Some(u) = opt.url.clone() {
            st.open_url(&u);
        }
        st.window.request_redraw();
        Ok(st)
    }

    // ---------- 场域上传 ----------

    /// 把当前标签的场域算好并上传 GPU。
    ///
    /// 链路：标签场域 → **与世界模型状态合成** → Gabor → **一致性调制** → 元素 → GPU。
    /// 「喜欢」经 `coherence_of` 得到相位一致性，落到三处：位置规则度 / 相位稳定 / 包络展宽。
    fn apply_active(&mut self) {
        let (field, quad) = {
            let t = &self.tabs[self.active];
            (t.field, t.quad)
        };
        let s = self.world.summary();
        // 画面呈现的是**当前世界模型的状态**（与当前页面的场域合成，世界权重 0.35）
        let fm = blend_with_world(&field, s.mean, 0.35, s.entries);
        self.coherence = coherence_of(&quad, &self.lib);
        let g = modulate_gabor(field_to_gabor_with(&fm, &self.lib), &quad);
        let elems = elements_of_co(&fm, &g, &quad, self.coherence, self.n_elements);
        upload_co(&self.queue, &self.pipeline, &elems, &g, self.coherence);
        self.clear = clear_color_of(&fm, &quad);
    }

    /// **动态 LOD**：按四元组决定精度档位（紧张高 → 更多细节；平静高 → 更省）。
    /// 带滞回（0.08）避免在阈值附近来回重建。
    fn update_lod(&mut self) {
        let q = self.tabs[self.active].quad;
        let target = lod_n_for(q.tension, q.calm);
        let cur_idx = self.n_elements.trailing_zeros() as i32;
        let tgt_idx = target.trailing_zeros() as i32;
        if (tgt_idx - cur_idx).abs() < 2 && target != self.n_elements {
            return; // 滞回：只有跨两档才切换，避免抖动
        }
        if target == self.n_elements {
            return;
        }
        let t0 = Instant::now();
        let old_bytes = field_buffer_bytes(self.n_elements);
        self.pipeline = build_pipeline(&self.device, target, self.config.format);
        self.n_elements = target;
        self.apply_active();
        println!(
            "[window] LOD 切换 {old_bytes} → {} 字节（n={target}，趟数={}），耗时 {:.2}ms",
            field_buffer_bytes(target),
            self.pipeline.sort_passes,
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }

    /// **世界模型更新**：切标签 / 取源码 / 调四元组都是一次交互。
    fn observe_world(&mut self, label: &str) {
        let t = &self.tabs[self.active];
        let (host, url, field, chars) = (short_host(&t.url), t.url.clone(), t.field, t.source.chars().count());
        let quad = t.quad;
        if url.starts_with("sample:") {
            self.world.observe_source(&host, &field, chars);
        } else {
            self.world.observe_page(&host, &url, &field, chars);
        }
        self.world.observe_interaction(label, &quad);
    }

    fn select_tab(&mut self, i: usize) {
        if i >= self.tabs.len() || i == self.active {
            return;
        }
        self.active = i;
        self.addr = self.tabs[i].url.clone();
        self.observe_world("切标签");
        self.apply_active();
        self.window.request_redraw();
    }

    fn select_sample(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.select_tab(i);
        }
    }

    fn new_tab(&mut self) {
        let (text_html, _) = sample_pages();
        let t = make_tab("新标签（纯文本页）", "sample:text", &text_html, self.baseline, &self.lib);
        self.tabs.push(t);
        self.active = self.tabs.len() - 1;
        self.addr = self.tabs[self.active].url.clone();
        self.apply_active();
    }

    fn close_tab(&mut self, i: usize) {
        if self.tabs.len() <= 1 || i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.addr = self.tabs[self.active].url.clone();
        self.apply_active();
    }

    // ---------- 取源码（后台线程，不冻结 UI）----------

    fn open_url(&mut self, raw: &str) {
        let Some(url) = safe_url(raw) else {
            self.status = format!("✗ 只接受 http/https 网址（伪协议一律不当作网址）：{raw}");
            return;
        };
        self.start_fetch(url, "手动打开");
    }

    fn refresh(&mut self) {
        let u = self.tabs[self.active].url.clone();
        if u.starts_with("sample:") {
            self.status = "当前是内置样本页；在地址栏输入网址可加载真实网页源码。".to_string();
            return;
        }
        self.start_fetch(u, "手动刷新");
    }

    fn start_fetch(&mut self, url: String, why: &str) {
        if self.fetch_rx.is_some() {
            self.status = "已有一次取源码在进行中，请稍候。".to_string();
            return;
        }
        self.status = format!("取源码中（{why}）：{url}");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = fetch_source(&url).map(|body| (url, body));
            let _ = tx.send(r);
        });
        self.fetch_rx = Some(rx);
    }

    fn pump_fetch(&mut self) {
        let Some(rx) = self.fetch_rx.as_ref() else { return };
        let got = match rx.try_recv() {
            Ok(v) => v,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.fetch_rx = None;
                self.status = "取源码线程异常结束".to_string();
                return;
            }
        };
        self.fetch_rx = None;
        match got {
            Ok((url, body)) => {
                let field = parse_source(&body, "");
                let title = format!("{} · {} 字", short_host(&url), body.chars().count());
                let tab = Tab {
                    title,
                    url: url.clone(),
                    text: text_digest(&body, 4000),
                    source: body,
                    field,
                    quad: self.tabs[self.active].quad,
                };
                // 当前标签是样本页时直接接管，避免标签越堆越多
                if self.tabs[self.active].url.starts_with("sample:") {
                    self.tabs[self.active] = tab;
                } else {
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                }
                self.addr = url.clone();
                self.last_source_at = Some(Instant::now());
                self.observe_world("取源码");
                self.apply_active();
                self.status = format!(
                    "已取源码并解析：{} ｜ 四场 地{:.2} 水{:.2} 火{:.2} 风{:.2} ｜ 置信度 {:.2}",
                    url, field.earth, field.water, field.fire, field.wind, field.confidence
                );
                // 同时打到 stdout，便于无人值守取证
                println!(
                    "[window] 取源码成功：{}（{} 字）｜ 四场 地{:.2} 水{:.2} 火{:.2} 风{:.2} ｜ 置信度 {:.2}",
                    url, self.tabs[self.active].source.chars().count(),
                    field.earth, field.water, field.fire, field.wind, field.confidence
                );
            }
            Err(e) => {
                self.status = format!("✗ 取源码失败：{e}");
            }
        }
        self.window.request_redraw();
    }

    // ---------- 下载（源码 + 场域 JSON + 画面 PPM）----------

    fn download(&mut self) -> Result<PathBuf, String> {
        let t = self.tabs[self.active].clone();
        let ts = stamp();
        let dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("downloads")
            .join(&ts);
        std::fs::create_dir_all(&dir).map_err(|e| format!("建目录失败：{e}"))?;
        // ① 网页源码
        std::fs::write(dir.join("page.html"), t.source.as_bytes())
            .map_err(|e| format!("写 page.html 失败：{e}"))?;
        // ② 场域状态 + 参数（便于复现 / 后续入库）
        let g = modulate_gabor(field_to_gabor_with(&t.field, &self.lib), &t.quad);
        let json = format!(
            "{{\n  \"url\": \"{}\",\n  \"title\": \"{}\",\n  \"chars\": {},\n  \"field\": {{\"earth\": {:.6}, \"water\": {:.6}, \"fire\": {:.6}, \"wind\": {:.6}, \"confidence\": {:.6}}},\n  \"gabor\": {{\"lambda\": {:.6}, \"theta\": {:.6}, \"sigma\": {:.6}, \"gamma\": {:.6}, \"psi\": {:.6}}},\n  \"quad\": {{\"tension\": {:.6}, \"calm\": {:.6}, \"liking\": {:.6}, \"safety\": {:.6}}},\n  \"probe\": \"{}\",\n  \"at\": \"{}\"\n}}\n",
            t.url.replace('"', "'"),
            t.title.replace('"', "'"),
            t.source.chars().count(),
            t.field.earth, t.field.water, t.field.fire, t.field.wind, t.field.confidence,
            g.lambda, g.theta, g.sigma, g.gamma, g.psi,
            t.quad.tension, t.quad.calm, t.quad.liking, t.quad.safety,
            probe_label(self.probe_mode, self.probe_reason),
            ts
        );
        std::fs::write(dir.join("field.json"), json.as_bytes())
            .map_err(|e| format!("写 field.json 失败：{e}"))?;
        // ②b 世界模型快照（可查询、可恢复）
        std::fs::write(dir.join("world.txt"), self.world.to_text().as_bytes())
            .map_err(|e| format!("写 world.txt 失败：{e}"))?;
        // ③ 当前画面（PPM P6，零依赖可打开）
        if let Some((rgb, w, h)) = self.capture_rgb() {
            let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
            ppm.extend_from_slice(&rgb);
            std::fs::write(dir.join("frame.ppm"), &ppm).map_err(|e| format!("写 frame.ppm 失败：{e}"))?;
        }
        Ok(dir)
    }

    /// 渲染一帧到离屏目标并回读为 RGB8（下载取帧用；**与上屏同一套管线**）。
    fn capture_rgb(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        // wgpu 23 的 TextureView 也不可 Clone → 直接借用（全是不可变借用，无冲突）
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("capture"),
        });
        record_field_passes(&self.pipeline, &mut enc, &self.offscreen_view, self.clear);
        self.queue.submit(Some(enc.finish()));

        let (w, h) = (self.config.width, self.config.height);
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("capture-copy"),
        });
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.offscreen,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &self.readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.bytes_per_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));

        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv();
        let data = slice.get_mapped_range();
        let ch = channel_offset(self.config.format);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            let row = (y * self.bytes_per_row) as usize;
            for x in 0..w {
                let p = row + (x * 4) as usize;
                // 统一转成 RGB 顺序（Bgra 时 R 在 +2）
                rgb.push(data[p + ch]);
                rgb.push(data[p + ((ch + 1) % 4)]);
                rgb.push(data[p + ((ch + 2) % 4)]);
            }
        }
        drop(data);
        self.readback.unmap();
        Some((rgb, w, h))
    }

    // ---------- 探测策略（默认被动；主动仅例外）----------

    fn update_probe(&mut self) {
        let (conf, quad) = {
            let t = &self.tabs[self.active];
            (t.field.confidence, t.quad)
        };
        let diag = diagnose(quad, self.baseline, &self.lib, conf);
        self.deviation = diag.deviation;
        self.dominant = diag.dominant.to_string();
        let stale = self.last_source_at.map(|t| t.elapsed().as_secs() > 90).unwrap_or(true);
        let d = decide_probe(conf, diag.deviation, stale);

        // 只有"被动 → 主动"的**翻转**才触发一次真实探测（取源码），且带冷却，避免探测风暴
        let flipped = d.mode == ProbeMode::Active && self.probe_mode == ProbeMode::Passive;
        let cooldown_ok = self
            .last_active_probe
            .map(|t| t.elapsed().as_secs() > 30)
            .unwrap_or(true);
        if flipped && cooldown_ok && self.fetch_rx.is_none() {
            let u = self.tabs[self.active].url.clone();
            if !u.starts_with("sample:") {
                self.last_active_probe = Some(Instant::now());
                let label = reason_label(d.reason);
                self.start_fetch(u, &format!("主动探测（{label}）"));
            }
        }
        self.probe_mode = d.mode;
        self.probe_reason = d.reason;
    }

    // ---------- UI ----------

    fn ui(&mut self, ctx: &egui::Context) {
        let mut act: Option<Action> = None;
        let mut quad_draft: Option<Quad> = None;

        egui::TopBottomPanel::top("chrome").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("网址");
                let w_avail = (ui.available_width() - 170.0).max(200.0);
                let resp = ui.add_sized(
                    [w_avail, 22.0],
                    egui::TextEdit::singleline(&mut self.addr).hint_text("https://… 或 sample:text"),
                );
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("打开").clicked() || enter {
                    act = Some(Action::Open(self.addr.clone()));
                }
                if ui.button("刷新").clicked() {
                    act = Some(Action::Refresh);
                }
                if ui.button("新标签").clicked() {
                    act = Some(Action::NewTab);
                }
            });
            ui.horizontal_wrapped(|ui| {
                for i in 0..self.tabs.len() {
                    let active = i == self.active;
                    let label = format!("{} {}", i + 1, clip(&self.tabs[i].title, 22));
                    if ui.selectable_label(active, label).clicked() {
                        act = Some(Action::SelectTab(i));
                    }
                    if self.tabs.len() > 1 && ui.small_button("×").clicked() {
                        act = Some(Action::CloseTab(i));
                    }
                }
            });
            ui.separator();
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(clip(&self.status, 150));
        });

        egui::SidePanel::right("field").default_width(340.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let (field, quad, title, url, text) = {
                    let t = &self.tabs[self.active];
                    (t.field, t.quad, t.title.clone(), t.url.clone(), t.text.clone())
                };
                ui.heading("场域读数");
                ui.label(&title);
                ui.label(egui::RichText::new(clip(&url, 44)).weak());
                ui.add_space(4.0);
                for (name, v) in [
                    ("地（结构度）", field.earth),
                    ("水（正文量）", field.water),
                    ("火（媒体密度）", field.fire),
                    ("风（交互/链接）", field.wind),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(name);
                        ui.add(
                            egui::ProgressBar::new(v as f32)
                                .desired_width(140.0)
                                .text(format!("{v:.2}")),
                        );
                    });
                }
                ui.label(format!("置信度 {:.2}", field.confidence));
                ui.add_space(6.0);

                let g = modulate_gabor(field_to_gabor_with(&field, &self.lib), &quad);
                ui.heading("映射参数（Gabor / DoG）");
                ui.label(format!("λ 波长 {:.3}　θ 方向 {:.2}", g.lambda, g.theta));
                ui.label(format!("σ 包络 {:.2}　γ 纵横 {:.2}　ψ 相位 {:.2}", g.sigma, g.gamma, g.psi));
                ui.add_space(6.0);

                ui.heading("四元组内在变量");
                let mut q = quad;
                ui.add(egui::Slider::new(&mut q.tension, 0.0..=1.0).text("紧张（多巴胺）"));
                ui.add(egui::Slider::new(&mut q.calm, 0.0..=1.0).text("平静（血清素）"));
                ui.add(egui::Slider::new(&mut q.liking, 0.0..=1.0).text("喜欢（内啡肽）"));
                ui.add(egui::Slider::new(&mut q.safety, 0.0..=1.0).text("安全（催产素）"));
                let changed = (q.tension - quad.tension).abs() > 1e-9
                    || (q.calm - quad.calm).abs() > 1e-9
                    || (q.liking - quad.liking).abs() > 1e-9
                    || (q.safety - quad.safety).abs() > 1e-9;
                if changed {
                    quad_draft = Some(q);
                }
                ui.checkbox(&mut self.natural_return, "无扰动时自然回归本底场（×e^-0.1）");
                ui.add_space(6.0);

                ui.heading("世界模型（L3）");
                let ws = self.world.summary();
                ui.label(format!(
                    "条目 {}　tick {}　主导 {}",
                    ws.entries,
                    ws.tick,
                    clip(&ws.dominant, 24)
                ));
                ui.label(format!(
                    "世界均值 地{:.2} 水{:.2} 火{:.2} 风{:.2}　一致性 {:.2}",
                    ws.mean[0], ws.mean[1], ws.mean[2], ws.mean[3], ws.coherence
                ));
                ui.label(
                    egui::RichText::new("画面 = 当前页面场域 ⊕ 世界模型状态（权重 0.35）").weak(),
                );
                ui.add_space(6.0);
                ui.heading("相位一致性（喜欢＝预测误差降低）");
                ui.label(format!("一致性 {:.3} → 位置规则度 ↑｜相位稳定 ↑｜包络展宽 ↑", self.coherence));
                ui.add_space(6.0);
                ui.heading("动态 LOD");
                ui.label(format!(
                    "活跃 splat {}　排序 {} 趟　场域缓冲 {} 字节",
                    self.n_elements,
                    self.pipeline.sort_passes,
                    field_buffer_bytes(self.n_elements)
                ));
                ui.label(egui::RichText::new("紧张↑ → 更多细节；平静↑ → 更省资源（128 ↔ 1024）").weak());
                ui.add_space(6.0);

                ui.heading("探测策略");
                ui.label(format!("模式：{}", probe_label(self.probe_mode, self.probe_reason)));
                ui.label(format!("偏离 {:.3}　主导 {}", self.deviation, self.dominant));
                ui.label(egui::RichText::new("默认被动（水面模式）；主动仅作例外且必带理由").weak());
                ui.add_space(6.0);

                if ui.button("下载（源码 + 场域 + 画面）").clicked() {
                    act = Some(Action::Download);
                }
                ui.horizontal(|ui| {
                    for i in 0..3 {
                        if ui.button(format!("样本{}", i + 1)).clicked() {
                            act = Some(Action::LoadSample(i));
                        }
                    }
                });
                ui.add_space(6.0);
                ui.separator();
                ui.label(egui::RichText::new("页面文本摘要").strong());
                ui.label(egui::RichText::new(clip(&text, 600)).monospace().weak());
            });
        });

        // 统一执行（避免在闭包里直接改 self 造成借用冲突）
        if let Some(q) = quad_draft {
            act = Some(Action::SetQuad(q));
        }
        if let Some(a) = act {
            self.apply_action(a);
        }
    }

    fn apply_action(&mut self, a: Action) {
        match a {
            Action::Open(u) => self.open_url(&u),
            Action::Refresh => self.refresh(),
            Action::NewTab => self.new_tab(),
            Action::CloseTab(i) => self.close_tab(i),
            Action::SelectTab(i) => self.select_tab(i),
            Action::LoadSample(i) => self.select_sample(i),
            Action::Download => match self.download() {
                Ok(p) => {
                    println!("[window] 下载完成：{}", p.display());
                    self.status = format!("已下载到：{}", p.display());
                }
                Err(e) => self.status = format!("✗ 下载失败：{e}"),
            },
            Action::SetQuad(q) => {
                self.tabs[self.active].quad = q;
                self.observe_world("调四元组");
                self.update_lod();
                self.apply_active();
            }
        }
        self.window.request_redraw();
    }

    // ---------- 每帧 ----------

    fn frame(&mut self) {
        if self.t_first.is_none() {
            self.t_first = Some(Instant::now());
        }
        self.pump_fetch();
        self.update_lod();
        self.update_probe();
        if self.natural_return {
            let q = self.tabs[self.active].quad;
            let r = regress(q, self.baseline);
            if (r.tension - q.tension).abs() > 1e-9 || (r.calm - q.calm).abs() > 1e-9 {
                self.tabs[self.active].quad = r;
                self.apply_active();
            }
        }

        // ---- egui：输入 → UI 树 → 上传纹理/缓冲 ----
        let ctx = self.egui_ctx.clone();
        let raw = self.egui_state.take_egui_input(&self.window);
        let out = ctx.run(raw, |c| self.ui(c));
        self.egui_state.handle_platform_output(&self.window, out.platform_output);
        let ppp = out.pixels_per_point;
        let jobs = ctx.tessellate(out.shapes, ppp);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ppp,
        };
        for (id, delta) in &out.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, delta);
        }

        // ---- 取帧 ----
        let ft = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(other) => {
                eprintln!("[window] 取帧失败：{other:?}");
                return;
            }
        };
        let view = ft.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // ---- 同一个 encoder：先场域 pass，再 egui pass（Load，叠在上面）----
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        self.egui_renderer
            .update_buffers(&self.device, &self.queue, &mut enc, &jobs, &screen);
        record_field_passes(&self.pipeline, &mut enc, &view, self.clear);
        {
            let rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // 保留场域画面，UI 叠上去
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            // egui-wgpu 0.30 要求 `RenderPass<'static>`；`forget_lifetime` 是**安全函数**（非取巧）
            let mut rp = rp.forget_lifetime();
            self.egui_renderer.render(&mut rp, &jobs, &screen);
        }
        self.queue.submit(Some(enc.finish()));
        for id in &out.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        ft.present();
        self.frames += 1;
        self.window.request_redraw(); // 持续渲染循环
    }

    fn resize(&mut self, w: u32, h: u32) {
        let max_dim = self.device.limits().max_texture_dimension_2d;
        let (w, h) = (w.min(max_dim).max(1), h.min(max_dim).max(1));
        if w == self.config.width && h == self.config.height {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        let (rb, bpr, os, osv) = make_targets(&self.device, self.config.format, w, h);
        self.readback = rb;
        self.bytes_per_row = bpr;
        self.offscreen = os;
        self.offscreen_view = osv;
        self.window.request_redraw();
    }

    fn should_finish(&self) -> bool {
        if self.ui_selftest {
            return self.frames > 8; // 等 UI 跑起来几帧后再验收
        }
        self.frames_target.map(|t| self.frames >= t).unwrap_or(false)
    }

    /// 无人值守验收：多标签切换 + 下载写盘（走与 UI 完全相同的代码路径）。
    fn ui_selftest(&mut self) -> i32 {
        println!("[ui-selftest] 标签数 {}", self.tabs.len());
        let before = self.active;
        self.select_tab(1);
        let switched = self.active == 1;
        println!("[ui-selftest] 多标签切换：{} → {}　{}", before, self.active, if switched { "OK" } else { "FAIL" });
        self.select_tab(0);

        // 地址栏输入校验（伪协议必须被拒）
        let rejected = safe_url("javascript:alert(1)").is_none()
            && safe_url("file:///c:/windows").is_none()
            && safe_url("  notaurl  ").is_none()
            && safe_url("https://example.com/a").is_some()
            && safe_url("http://192.168.1.3/").is_some();
        println!("[ui-selftest] 地址栏协议校验（仅 http/https）：{}", if rejected { "OK" } else { "FAIL" });

        match self.download() {
            Ok(p) => {
                let entries: Vec<(String, u64)> = std::fs::read_dir(&p)
                    .map(|d| {
                        d.filter_map(|e| e.ok())
                            .map(|e| {
                                (
                                    e.file_name().to_string_lossy().to_string(),
                                    e.metadata().map(|m| m.len()).unwrap_or(0),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let detail = entries
                    .iter()
                    .map(|(n, s)| format!("{n}={s}B"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("[ui-selftest] 下载目录：{}（{} 个文件：{}）", p.display(), entries.len(), detail);
                let ok = entries.len() >= 3 && entries.iter().all(|(_, s)| *s > 0);
                println!("[ui-selftest] 下载写盘（源码 + 场域JSON + 画面PPM）：{}", if ok { "OK" } else { "FAIL" });
                if ok && switched && rejected {
                    0
                } else {
                    1
                }
            }
            Err(e) => {
                println!("[ui-selftest] ✗ 下载失败：{e}");
                1
            }
        }
    }

    /// 自动验收：帧率（呈现口径 + 管线裸口径）＋ **回读上屏像素** 做方差/pHash 判定。
    fn finish(&mut self) -> i32 {
        let mut selftest_fail = false;
        if self.ui_selftest && !self.ui_selftest_done {
            self.ui_selftest_done = true;
            selftest_fail = self.ui_selftest() != 0;
        }

        let secs = self
            .t_first
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
            .max(1e-9);
        let present_fps = self.frames as f64 / secs;

        const RAW: u32 = 60;
        let t0 = Instant::now();
        for _ in 0..RAW {
            let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("raw"),
            });
            record_field_passes(&self.pipeline, &mut enc, &self.offscreen_view, self.clear);
            self.queue.submit(Some(enc.finish()));
        }
        let _ = self.device.poll(wgpu::Maintain::Wait);
        let raw_fps = RAW as f64 / t0.elapsed().as_secs_f64();

        let (src, got) = if self.surface_copy_src {
            ("surface（真实上屏纹理）", self.read_target(true))
        } else {
            ("离屏（该 surface 不支持 COPY_SRC，退化为同管线离屏回读）", self.read_target(false))
        };

        println!(
            "[window] 帧率：呈现口径 {:.1} FPS（{} 帧 / {:.2}s）｜管线裸口径 {:.1} FPS（{} splat，排序 {} 趟）",
            present_fps, self.frames, secs, raw_fps, self.pipeline.n, self.pipeline.sort_passes
        );
        let mut ok = !selftest_fail;
        println!(
            "[window] 验收① 帧率 > 30 FPS：呈现 {:.1}｜裸 {:.1} → {}",
            present_fps,
            raw_fps,
            if present_fps > 30.0 || raw_fps > 30.0 { "PASS" } else { "FAIL" }
        );
        if !(present_fps > 30.0 || raw_fps > 30.0) {
            ok = false;
        }
        match got {
            Some((s, ph)) => {
                println!(
                    "[window] 回读来源：{src}｜均值={:.1} 方差={:.1} 非纯黑非纯白={} pHash={:016x}",
                    s.mean, s.var, s.ok, ph
                );
                println!(
                    "[window] 验收② 画面非纯黑非纯白且方差>1：{}（方差 {:.1}）",
                    if s.ok { "PASS" } else { "FAIL" },
                    s.var
                );
                if !s.ok {
                    ok = false;
                }
            }
            None => {
                println!("[window] 验收② 回读失败（拿不到像素）→ FAIL");
                ok = false;
            }
        }
        if self.ui_selftest {
            println!("[window] 验收③ 多标签切换 + 下载写盘：{}", if selftest_fail { "FAIL" } else { "PASS" });
        }
        println!("[window] 结论：{}", if ok { "PASS" } else { "FAIL" });
        if ok {
            0
        } else {
            1
        }
    }

    /// 渲染一帧到指定目标并回读像素（`from_surface=true` 时读 surface 纹理本身）。
    fn read_target(&mut self, from_surface: bool) -> Option<(crate::PxStats, u64)> {
        // wgpu 23 的 `wgpu::Texture` **不是 Clone**；此处直接**借用**目标纹理——不复制、不 transmute。
        let held = if from_surface {
            match self.surface.get_current_texture() {
                Ok(f) => Some(f),
                Err(_) => return None,
            }
        } else {
            None
        };
        let tex: &wgpu::Texture = match &held {
            Some(f) => &f.texture,
            None => &self.offscreen,
        };
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback-render"),
        });
        record_field_passes(&self.pipeline, &mut enc, &view, self.clear);
        self.queue.submit(Some(enc.finish()));

        let (w, h) = (self.config.width, self.config.height);
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &self.readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.bytes_per_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));

        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv();
        let data = slice.get_mapped_range();
        let ch = channel_offset(self.config.format);
        let mut px = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            let row = (y * self.bytes_per_row) as usize;
            for x in 0..w {
                px.push(data[row + (x * 4) as usize + ch]);
            }
        }
        drop(data);
        self.readback.unmap();
        if let Some(f) = held {
            f.present(); // 必须呈现，否则该 swapchain 图像一直被占用
        }
        Some((crate::pixel_stats(&px), crate::dhash_8x8(&px, w, h)))
    }
}

// ===================== 小工具 =====================

fn channel_offset(f: wgpu::TextureFormat) -> usize {
    match f {
        wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm => 2,
        _ => 0,
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        s.chars().take(n).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

fn short_host(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    rest.split('/').next().unwrap_or(rest).to_string()
}

fn reason_label(r: Option<ProbeReason>) -> &'static str {
    match r {
        Some(ProbeReason::StaleSignal) => "信号陈旧",
        Some(ProbeReason::LowConfidence) => "置信度低",
        Some(ProbeReason::HighDeviation) => "偏离过大",
        None => "—",
    }
}

fn probe_label(m: ProbeMode, r: Option<ProbeReason>) -> String {
    match m {
        ProbeMode::Passive => "被动（水面模式）".to_string(),
        ProbeMode::Active => format!("主动（例外；理由：{}）", reason_label(r)),
    }
}

fn stamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{now}")
}

/// 建回读缓冲与离屏目标（尺寸随窗口变化时重建）。
fn make_targets(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
) -> (wgpu::Buffer, u32, wgpu::Texture, wgpu::TextureView) {
    let bytes_per_row = ((w * 4 + 255) / 256) * 256; // 256 对齐（copy_texture_to_buffer 要求）
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("window-readback"),
        size: (bytes_per_row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let offscreen = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("window-offscreen"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = offscreen.create_view(&wgpu::TextureViewDescriptor::default());
    (readback, bytes_per_row, offscreen, view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> GeneLibrary {
        let mut l = GeneLibrary::new();
        seed_gabor_into(&mut l);
        l
    }

    #[test]
    fn url_whitelist_only_http_https() {
        assert!(safe_url("https://example.com/a").is_some());
        assert!(safe_url("http://192.168.1.3:3000/").is_some());
        for bad in [
            "javascript:alert(1)",
            "data:text/html,<b>x</b>",
            "vbscript:msgbox",
            "file:///c:/windows",
            "about:blank",
            "ftp://x/",
            "notaurl",
            "https://",
        ] {
            assert!(safe_url(bad).is_none(), "{bad} 不该被当作网址");
        }
    }

    #[test]
    fn text_digest_strips_tags_and_scripts() {
        let html = "<html><head><style>a{color:red}</style><script>var x=1;</script></head><body><h1>标题</h1><p>正文一</p><p>正文二</p></body></html>";
        let t = text_digest(html, 100);
        assert!(t.contains("标题") && t.contains("正文一"), "应保留可见文本：{t}");
        assert!(!t.contains("var x"), "脚本内容不该出现：{t}");
        assert!(!t.contains("color:red"), "样式内容不该出现：{t}");
    }

    #[test]
    fn tabs_reflect_field_difference() {
        let l = lib();
        let qn = Quad { tension: 0.2, calm: 0.6, liking: 0.5, safety: 0.6 };
        let (t, i) = sample_pages();
        let a = make_tab("纯文本页", "sample:text", &t, qn, &l);
        let b = make_tab("纯图片页", "sample:image", &i, qn, &l);
        assert!(a.field.water > b.field.water, "文本页水应更高");
        assert!(b.field.fire > a.field.fire, "图片页火应更高");
    }

    #[test]
    fn probe_label_covers_all_reasons() {
        assert!(probe_label(ProbeMode::Passive, None).contains("被动"));
        for r in [ProbeReason::StaleSignal, ProbeReason::LowConfidence, ProbeReason::HighDeviation] {
            let s = probe_label(ProbeMode::Active, Some(r));
            assert!(s.contains("主动") && s.contains(reason_label(Some(r))), "{s}");
        }
    }

    #[test]
    fn readback_row_alignment_is_256() {
        for w in [1u32, 100, 768, 1024, 1366, 2134] {
            let bpr = ((w * 4 + 255) / 256) * 256;
            assert_eq!(bpr % 256, 0);
            assert!(bpr >= w * 4);
        }
    }
}
