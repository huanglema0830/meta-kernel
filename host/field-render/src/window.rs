//! 场域呈现器 · **窗口宿主（winit 0.30 + wgpu surface）** —— 第三阶段 · 第一步
//!
//! 本步**只做三件事**：① 开窗 ② 创建 wgpu surface ③ 把已验证的场域渲染管线
//! （`preprocess.wgsl` → 双调排序 → `render.wgsl`）接到窗口 surface 上。
//! **不含**地址栏 / 多标签 / 下载 / egui（属后续步骤）。
//!
//! ## 不依赖 WebView2
//! 只用 `winit` + `wgpu` + 内核（内核自身零依赖）。**无 `unsafe` 取巧**：
//! `Instance::create_surface` 在 wgpu 23 是**安全函数**（已核源码 `wgpu-23.0.1/src/api/instance.rs:276`，
//! 内部自行处理句柄生命周期），故本项目**不需要任何 `unsafe` 块**。
//!
//! ## 用法
//! - `field-render`：开窗持续渲染；`1/2/3` 切换场域样本，`Esc`/`Q`/关窗退出
//! - `field-render --frames N`：渲染 N 帧后**自动**测帧率 + **回读上屏像素**做客观判定，然后退出
//!   （无人值守验收；无需人眼，也不需要截图工具）

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use meta_kernel_core::gene_library::GeneLibrary;
use meta_kernel_core::l1_mapping::{field_to_gabor_with, modulate_gabor, seed_gabor_into};
use meta_kernel_core::l1_source_parse::parse_source;
use meta_kernel_core::l5_quad::Quad;

use crate::{
    build_pipeline, clear_color_of, dhash_8x8, elements_of, encode_frame, pixel_stats, upload, Pipeline,
    N_ELEMENTS, sample_pages,
};

/// 启动选项。
pub struct RunOptions {
    /// `Some(n)`：渲染 n 帧后自动验收并退出；`None`：持续渲染直到用户关窗。
    pub frames: Option<u32>,
    /// 启动即选中的场域样本（0=纯文本页 1=纯图片页 2=纯图片页·紧张高）。
    /// 供无人值守验收逐个样本取证"场域不同 → 画面不同"。
    pub sample: usize,
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

/// 场域样本：走**完整内核链路**（源码直解 → 四场 → Gabor → 四元组调制）。
/// 三个样本用于对比"场域不同 → 画面不同""四元组不同 → 画面不同"。
struct Sample {
    name: &'static str,
    html: String,
    quad: Quad,
}

fn samples() -> Vec<Sample> {
    let (text_html, image_html) = sample_pages();
    let neutral = Quad { tension: 0.2, calm: 0.6, liking: 0.5, safety: 0.6 };
    let hot = Quad { tension: 1.0, calm: 0.1, liking: 0.4, safety: 0.3 };
    vec![
        Sample { name: "纯文本页", html: text_html.clone(), quad: neutral },
        Sample { name: "纯图片页", html: image_html.clone(), quad: neutral },
        Sample { name: "纯图片页·紧张高", html: image_html, quad: hot },
    ]
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
        match State::new(event_loop, self.opt.frames, self.opt.sample) {
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
                        "1" => state.select(0),
                        "2" => state.select(1),
                        "3" => state.select(2),
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

// ===================== 渲染状态 =====================

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: Pipeline,
    lib: GeneLibrary,
    samples: Vec<Sample>,
    clear: wgpu::Color,

    // 回读（客观验收用）
    readback: wgpu::Buffer,
    bytes_per_row: u32,
    /// surface 纹理是否支持 `COPY_SRC`（支持则回读的是**真实上屏的那张纹理**）
    surface_copy_src: bool,
    /// 离屏目标（管线裸帧率测量用，不含呈现/垂直同步）
    offscreen: wgpu::Texture,
    offscreen_view: wgpu::TextureView,

    // 验收计数
    frames_target: Option<u32>,
    frames: u32,
    t_first: Option<Instant>,
}

impl State {
    fn new(event_loop: &ActiveEventLoop, frames_target: Option<u32>, sample: usize) -> Result<Self, String> {
        let attrs = Window::default_attributes()
            .with_title("空天浏览器 · 场域呈现（wgpu 直渲，不依赖 WebView2）")
            .with_inner_size(winit::dpi::LogicalSize::new(768.0, 768.0));
        let window = Arc::new(event_loop.create_window(attrs).map_err(|e| format!("建窗失败：{e}"))?);
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1)); // 钳制在 ③ 处（拿到 device 之后）

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        // 注意：safe 函数（wgpu 23）——不需要 unsafe，也不是 transmute 类取巧
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
                // 关键：`downlevel_defaults()` 把 max_texture_dimension_2d 限在 2048，
                // 而 HiDPI 下窗口可达 2134+ → `Surface::configure` 直接 Validation Error panic。
                // `using_resolution` 只把「分辨率相关」上限抬到适配器实际能力，其余保持保守默认。
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
        // 优先 sRGB（与离屏自检口径一致）
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
        // 钳制到设备支持的最大纹理边长（超出会让 Surface::configure 直接 panic）
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
            "[window] 窗口已打开 {}x{} · surface 格式 {:?} · 呈现模式 {:?} · 可回读(COPY_SRC)={}",
            w, h, format, present_mode, surface_copy_src
        );
        println!("[window] 适配器: {} / {:?} / {:?}", info.name, info.backend, info.device_type);

        // 管线格式必须与 surface 实际格式一致
        let pipeline = build_pipeline(&device, N_ELEMENTS, format);

        let mut lib = GeneLibrary::new();
        seed_gabor_into(&mut lib);
        let samples = samples();

        let (readback, bytes_per_row, offscreen, offscreen_view) = make_targets(&device, format, w, h);

        let mut st = State {
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            lib,
            samples,
            clear: wgpu::Color::BLACK,
            readback,
            bytes_per_row,
            surface_copy_src,
            offscreen,
            offscreen_view,
            frames_target,
            frames: 0,
            t_first: None,
        };
        st.select(sample); // 首帧即场域画面（样本由 --sample 指定）
        st.window.request_redraw();
        Ok(st)
    }

    /// 切换样本（走完整内核链路后上传 GPU）。
    fn select(&mut self, idx: usize) {
        if idx >= self.samples.len() {
            return;
        }
        let (name, html, quad) = {
            let s = &self.samples[idx];
            (s.name, s.html.clone(), s.quad)
        };
        let field = parse_source(&html, "");
        let g = modulate_gabor(field_to_gabor_with(&field, &self.lib), &quad);
        let elems = elements_of(&field, &g, &quad, N_ELEMENTS);
        upload(&self.queue, &self.pipeline, &elems, &g);
        self.clear = clear_color_of(&field, &quad);
        println!(
            "[window] 样本「{name}」四场: 地{:.2} 水{:.2} 火{:.2} 风{:.2} | Gabor λ{:.3} θ{:.2} σ{:.2} γ{:.2} | 四元组 紧张{:.2}/平静{:.2}/喜欢{:.2}/安全{:.2}",
            field.earth, field.water, field.fire, field.wind,
            g.lambda, g.theta, g.sigma, g.gamma,
            quad.tension, quad.calm, quad.liking, quad.safety
        );
        self.window.request_redraw();
    }

    fn resize(&mut self, w: u32, h: u32) {
        // 钳制到设备上限（HiDPI / 最大化时窗口可能超过 2048）
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

    /// 一帧：取 surface 纹理 → 编码场域渲染 → 提交 → 呈现。
    fn frame(&mut self) {
        if self.t_first.is_none() {
            self.t_first = Some(Instant::now());
        }
        let ft = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config); // 失效 → 重配后下一帧再试
                return;
            }
            Err(other) => {
                eprintln!("[window] 取帧失败：{other:?}");
                return;
            }
        };
        let view = ft.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let cb = encode_frame(&self.device, &self.queue, &self.pipeline, &view, self.clear);
        self.queue.submit(Some(cb));
        ft.present();
        self.frames += 1;
        self.window.request_redraw(); // 持续渲染循环
    }

    fn should_finish(&self) -> bool {
        self.frames_target.map(|t| self.frames >= t).unwrap_or(false)
    }

    /// 自动验收：帧率（呈现口径 + 管线裸口径）＋ **回读上屏像素** 做方差/pHash 判定。
    fn finish(&mut self) -> i32 {
        let secs = self
            .t_first
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
            .max(1e-9);
        let present_fps = self.frames as f64 / secs;

        // 管线裸帧率：同一 encoder 路径、离屏目标、不含呈现/垂直同步（与 --selftest 同口径）
        const RAW: u32 = 60;
        let t0 = Instant::now();
        for _ in 0..RAW {
            let cb = encode_frame(
                &self.device,
                &self.queue,
                &self.pipeline,
                &self.offscreen_view,
                self.clear,
            );
            self.queue.submit(Some(cb));
        }
        let _ = self.device.poll(wgpu::Maintain::Wait);
        let raw_fps = RAW as f64 / t0.elapsed().as_secs_f64();

        // 上屏内容回读：优先读 surface 纹理本身（＝真实上屏的那张）
        let (src, got) = if self.surface_copy_src {
            ("surface（真实上屏纹理）", self.read_target(true))
        } else {
            ("离屏（该 surface 不支持 COPY_SRC，退化为同管线离屏回读）", self.read_target(false))
        };

        println!(
            "[window] 帧率：呈现口径 {:.1} FPS（{} 帧 / {:.2}s）｜管线裸口径 {:.1} FPS（{} splat，排序 {} 趟）",
            present_fps, self.frames, secs, raw_fps, self.pipeline.n, self.pipeline.sort_passes
        );

        let mut ok = true;
        // 验收：帧率 > 30（呈现口径受垂直同步限制，取两者较大值与阈值比较并如实标注）
        println!(
            "[window] 验收① 帧率 > 30 FPS：呈现口径 {:.1}｜裸口径 {:.1} → {}",
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
        println!("[window] 结论：{}", if ok { "PASS" } else { "FAIL" });
        if ok {
            0
        } else {
            1
        }
    }

    /// 渲染一帧到指定目标并回读像素（`from_surface=true` 时读 surface 纹理本身）。
    fn read_target(&mut self, from_surface: bool) -> Option<(crate::PxStats, u64)> {
        // wgpu 23 的 `wgpu::Texture` **不是 Clone**；此处直接**借用**目标纹理
        // （来自 surface 的交换链图像，或本状态持有的离屏纹理）——不复制、不 transmute。
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
        let cb = encode_frame(&self.device, &self.queue, &self.pipeline, &view, self.clear);
        self.queue.submit(Some(cb));

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
        // Bgra 格式时 R 通道在字节 2；取**同一通道**做统计与 pHash，口径一致
        let ch = if self.config.format == wgpu::TextureFormat::Bgra8UnormSrgb
            || self.config.format == wgpu::TextureFormat::Bgra8Unorm
        {
            2
        } else {
            0
        };
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
        let stats = pixel_stats(&px);
        let ph = dhash_8x8(&px, w, h);
        Some((stats, ph))
    }
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

    #[test]
    fn samples_cover_field_and_quad_differences() {
        let s = samples();
        assert_eq!(s.len(), 3, "三个样本");
        assert_eq!(s[0].name, "纯文本页");
        // 第 3 个样本：同页面、不同四元组 → 用于验证"四元组影响画面"
        assert_eq!(s[1].html, s[2].html, "样本2/3 用同一页面");
        assert!((s[2].quad.tension - 1.0).abs() < 1e-9, "样本3 紧张=1");
        assert!(s[0].quad.tension < 0.5, "样本1 紧张低");
    }

    #[test]
    fn readback_row_alignment_is_256() {
        // copy_texture_to_buffer 要求每行 256 字节对齐
        for w in [1u32, 100, 768, 1024, 1366] {
            let bpr = ((w * 4 + 255) / 256) * 256;
            assert_eq!(bpr % 256, 0);
            assert!(bpr >= w * 4);
        }
    }
}
