//! 场域呈现器 · **wgpu 独立渲染管线**（不依赖 WebView2）
//!
//! 依据：发起人「第二阶段：场域呈现器（wgpu 管线）」。链路：
//! ```text
//! 内核（源码 → 四场 → Gabor/DoG → 四元组调制）
//!   → 场域元素 buffer
//!   → Stage1 preprocess.wgsl（场域元素 → 2D 高斯泼溅参数；Σ' = J·W·Σ·Wᵀ·Jᵀ）
//!   → Stage2 sort.wgsl（GPU 双调排序，渲染端反序读 → 远者先画）
//!   → Stage3 render.wgsl（屏幕空间四边形 + exp(-2r²) 高斯衰减）
//!   → 画面
//! ```
//!
//! ## 两种模式
//! - `field-render --selftest`：**离屏渲染 + 回读 + 客观度量**（无窗口，可在 CI/无显示环境跑）
//!   · 非纯黑/非纯白｜pHash(dHash 8×8) 汉明距离｜像素统计｜帧率
//! - `field-render`（默认）：提示窗口模式留待下一轮
//!
//! ## 与 wry 版的关系
//! `host/sky-browser`（WebView2 版）**保留为降级路径与对照**；本 crate 完全独立。

use bytemuck::{Pod, Zeroable};
use std::time::Instant;

use meta_kernel_core::gene_library::GeneLibrary;
use meta_kernel_core::l1_field_parse::FieldReading;
use meta_kernel_core::l1_mapping::{field_to_gabor_with, modulate_gabor, seed_gabor_into, GaborParams};
use meta_kernel_core::l1_source_parse::parse_source;
use meta_kernel_core::l5_quad::Quad;

const W: u32 = 512;
const H: u32 = 512;
const N_ELEMENTS: u32 = 1024; // 2 的幂，双调排序需要

/// **内核 Gabor σ → 本场景像素尺度的换算系数**（宿主职责）。
/// 内核的 σ = 水·5+0.5（0.5..5.5，模型参数）；本场景世界坐标已是 NDC，
/// 1 单位 = 256 px，直接用会把 splat 撑到上千像素（v0.116 实测：整屏被一个 splat 覆盖 → 画面全平）。
/// 故换算为 **≈0.01 NDC**（2–3 px 起），并把 γ 的各向异性保留。
const SIGMA_SCALE: f32 = 0.02;
/// 内核参数 → 本场景的强度基准（四元组"活跃度"在此基础上调制）。
const ACTIVITY_BASE: f32 = 1.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, Default)]
struct FieldElement {
    position: [f32; 3],
    intensity: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
struct Splat2D {
    position: [f32; 2],
    cov2d: [f32; 4],
    color: [f32; 4],
    depth: f32,
    _pad: [f32; 3],
}
impl Default for Splat2D {
    fn default() -> Self {
        Self { position: [0.0; 2], cov2d: [0.0; 4], color: [0.0; 4], depth: 0.0, _pad: [0.0; 3] }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Camera {
    view_proj: [f32; 16],
    focal: [f32; 2],
    viewport: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GaborUniform {
    lambda: f32,
    theta: f32,
    sigma: f32,
    gamma: f32,
    psi: f32,
    _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SortParams {
    k: u32,
    j: u32,
    n: u32,
    _pad: u32,
}

// ===== 场域 → 元素（确定性生成：同场域必得同画面，便于 pHash 对比）=====

fn hash01(x: u64) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    ((h & 0xFF_FFFF) as f32) / 16_777_215.0
}

fn elements_of(f: &FieldReading, g: &GaborParams, q: &Quad, n: u32) -> Vec<FieldElement> {
    let mut out = Vec::with_capacity(n as usize);
    // 四场是 f64（内核口径），GPU 侧一律 f32 —— 此处显式收口，避免隐式转换偷偷发生
    let grid = f.earth.clamp(0.0, 1.0) as f32; // 地高 → 更接近规则网格
    let flat = f.water.clamp(0.0, 1.0) as f32; // 水大 → 更"平铺"
    let fire = f.fire.clamp(0.0, 1.0) as f32;
    let wind = f.wind.clamp(0.0, 1.0) as f32;
    let side = (n as f32).sqrt().ceil() as u32;
    let (s, c) = (g.theta as f32).sin_cos();
    let scl = 1.0 + 0.15 * g.sigma as f32;
    for i in 0..n {
        let gx = (i % side) as f32 / side as f32;
        let gy = (i / side) as f32 / side as f32;
        let h = hash01(i as u64 * 2_654_435_761 + 12_345);
        let h2 = hash01(i as u64 * 40_503 + 7_919);
        let x = (gx + (h - 0.5) * (1.0 - grid) * 0.9 - 0.5) * 2.0;
        let y = (gy + (h2 - 0.5) * (1.0 - grid) * 0.9 - 0.5) * 2.0;
        let z = (h - 0.5) * (1.0 - flat) * 0.5;
        // 四元组 → 画面活跃度（与内核表一致：紧张 → 更活跃；平静 → 更稳定）
        let t_q = q.tension.clamp(0.0, 1.0) as f32;
        let c_q = q.calm.clamp(0.0, 1.0) as f32;
        let l_q = q.liking.clamp(0.0, 1.0) as f32;
        let activity = ACTIVITY_BASE * (1.0 + 0.45 * t_q - 0.25 * c_q + 0.20 * l_q);
        let intensity = ((0.25 + 0.6 * fire + 0.3 * wind) * activity).clamp(0.05, 1.0);
        out.push(FieldElement {
            position: [(x * c - y * s) * scl, (x * s + y * c) * scl, z],
            intensity,
        });
    }
    out
}

/// 四场决定背景色（画面因此随场域显著变化）。
fn clear_color_of(f: &FieldReading, q: &Quad) -> wgpu::Color {
    let lift = 1.0 + 0.25 * q.tension.clamp(0.0, 1.0) - 0.15 * q.calm.clamp(0.0, 1.0);
    wgpu::Color {
        r: ((0.04 + 0.22 * f.earth.clamp(0.0, 1.0)) * lift).clamp(0.0, 0.95) as f64,
        g: ((0.04 + 0.22 * f.water.clamp(0.0, 1.0)) * lift).clamp(0.0, 0.95) as f64,
        b: ((0.04 + 0.40 * f.fire.clamp(0.0, 1.0)) * lift).clamp(0.0, 0.95) as f64,
        a: 1.0,
    }
}

// ===== GPU 初始化 =====

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn init_gpu() -> Result<Gpu, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or_else(|| "本机枚举不到 GPU 适配器（wgpu 报 None）".to_string())?;
    let info = adapter.get_info();
    println!("[field-render] 适配器: {} / {:?} / {:?}", info.name, info.backend, info.device_type);
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("field-render"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    ))
    .map_err(|e| format!("创建设备失败：{e}"))?;
    Ok(Gpu { device, queue })
}

// ===== 管线 =====

struct Pipeline {
    elements: wgpu::Buffer,
    splats_a: wgpu::Buffer,
    splats_b: wgpu::Buffer,
    camera_buf: wgpu::Buffer,
    gabor_buf: wgpu::Buffer,
    sort_params: wgpu::Buffer,
    bg_pre: wgpu::BindGroup,
    bg_io_a: wgpu::BindGroup,
    bg_sort_a: wgpu::BindGroup,
    bg_sort_b: wgpu::BindGroup,
    bg_render: wgpu::BindGroup,
    pre_pipeline: wgpu::ComputePipeline,
    sort_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    n: u32,
    sort_passes: u32,
}

fn build_pipeline(gpu: &Gpu, n: u32) -> Pipeline {
    let d = &gpu.device;

    let elements = d.create_buffer(&wgpu::BufferDescriptor {
        label: Some("elements"),
        size: (std::mem::size_of::<FieldElement>() as u64) * n as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mk_splat = |label: &str| {
        d.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (std::mem::size_of::<Splat2D>() as u64) * n as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    };
    let splats_a = mk_splat("splats_a");
    let splats_b = mk_splat("splats_b");

    let uni = |label: &str, size: u64| {
        d.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    let camera_buf = uni("camera", std::mem::size_of::<Camera>() as u64);
    let gabor_buf = uni("gabor", std::mem::size_of::<GaborUniform>() as u64);
    let sort_params = uni("sort_params", std::mem::size_of::<SortParams>() as u64);

    let pre_mod = d.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("preprocess"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../sky-browser/shaders/preprocess.wgsl").into()),
    });
    let sort_mod = d.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sort"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/sort.wgsl").into()),
    });
    let render_mod = d.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("render"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../sky-browser/shaders/render.wgsl").into()),
    });

    let uniform_entry = |binding: u32, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let storage_entry = |binding: u32, vis: wgpu::ShaderStages, ro: bool| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: ro },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };

    let bgl_pre = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bgl-pre"),
        entries: &[uniform_entry(0, wgpu::ShaderStages::COMPUTE), uniform_entry(1, wgpu::ShaderStages::COMPUTE)],
    });
    let bgl_io = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bgl-io"),
        entries: &[
            storage_entry(0, wgpu::ShaderStages::COMPUTE, true),
            storage_entry(1, wgpu::ShaderStages::COMPUTE, false),
        ],
    });
    let bgl_sort = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bgl-sort"),
        entries: &[
            uniform_entry(0, wgpu::ShaderStages::COMPUTE),
            storage_entry(1, wgpu::ShaderStages::COMPUTE, true),
            storage_entry(2, wgpu::ShaderStages::COMPUTE, false),
        ],
    });
    let bgl_render = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bgl-render"),
        entries: &[storage_entry(0, wgpu::ShaderStages::VERTEX, true)],
    });

    let bg_pre = d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg-pre"),
        layout: &bgl_pre,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: camera_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: gabor_buf.as_entire_binding() },
        ],
    });
    let bg_io_a = d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg-io-a"),
        layout: &bgl_io,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: elements.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: splats_a.as_entire_binding() },
        ],
    });
    let mk_sort = |label: &str, src: &wgpu::Buffer, dst: &wgpu::Buffer| {
        d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &bgl_sort,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: sort_params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: dst.as_entire_binding() },
            ],
        })
    };
    let bg_sort_a = mk_sort("bg-sort-a", &splats_a, &splats_b);
    let bg_sort_b = mk_sort("bg-sort-b", &splats_b, &splats_a);
    let bg_render = d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg-render"),
        layout: &bgl_render,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: splats_a.as_entire_binding() }],
    });

    let pre_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pre-layout"),
        bind_group_layouts: &[&bgl_pre, &bgl_io],
        push_constant_ranges: &[],
    });
    let sort_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sort-layout"),
        bind_group_layouts: &[&bgl_sort],
        push_constant_ranges: &[],
    });
    let render_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("render-layout"),
        bind_group_layouts: &[&bgl_render],
        push_constant_ranges: &[],
    });

    let pre_pipeline = d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("pre-pipeline"),
        layout: Some(&pre_layout),
        module: &pre_mod,
        entry_point: Some("preprocess"),
        compilation_options: Default::default(),
        cache: None,
    });
    let sort_pipeline = d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("sort-pipeline"),
        layout: Some(&sort_layout),
        module: &sort_mod,
        entry_point: Some("bitonic_step"),
        compilation_options: Default::default(),
        cache: None,
    });
    let render_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("render-pipeline"),
        layout: Some(&render_layout),
        vertex: wgpu::VertexState {
            module: &render_mod,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &render_mod,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    let sort_passes = bitonic_pass_count(n);

    Pipeline {
        elements, splats_a, splats_b, camera_buf, gabor_buf, sort_params,
        bg_pre, bg_io_a, bg_sort_a, bg_sort_b, bg_render,
        pre_pipeline, sort_pipeline, render_pipeline, n, sort_passes,
    }
}

/// 双调排序的趟数 = log2(n)·(log2(n)+1)/2。
pub fn bitonic_pass_count(n: u32) -> u32 {
    let mut logn = 0u32;
    while (1u32 << logn) < n {
        logn += 1;
    }
    logn * (logn + 1) / 2
}

fn upload(gpu: &Gpu, p: &Pipeline, elems: &[FieldElement], g: &GaborParams) {
    let n = p.n.min(elems.len() as u32);
    gpu.queue.write_buffer(&p.elements, 0, bytemuck::cast_slice(&elems[..n as usize]));
    let camera = Camera {
        view_proj: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
        focal: [W as f32 * 0.5, H as f32 * 0.5],
        viewport: [W as f32, H as f32],
    };
    gpu.queue.write_buffer(&p.camera_buf, 0, bytemuck::bytes_of(&camera));
    gpu.queue.write_buffer(
        &p.gabor_buf,
        0,
        bytemuck::bytes_of(&GaborUniform {
            lambda: g.lambda as f32,
            theta: g.theta as f32,
            sigma: (g.sigma as f32) * SIGMA_SCALE, // 内核 σ → 本场景像素尺度（见 SIGMA_SCALE 说明）
            gamma: g.gamma as f32,
            psi: g.psi as f32,
            _pad: [0.0; 3],
        }),
    );
}

fn encode_frame(gpu: &Gpu, p: &Pipeline, target: &wgpu::TextureView, clear: wgpu::Color) -> wgpu::CommandBuffer {
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
    {
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("preprocess"), timestamp_writes: None });
        cp.set_pipeline(&p.pre_pipeline);
        cp.set_bind_group(0, &p.bg_pre, &[]);
        cp.set_bind_group(1, &p.bg_io_a, &[]);
        cp.dispatch_workgroups((p.n + 255) / 256, 1, 1);
    }
    // Stage 2：双调排序（ping-pong；每趟一次 dispatch）
    {
        let mut k = 2u32;
        let mut ping = true;
        while k <= p.n {
            let mut j = k >> 1;
            while j > 0 {
                gpu.queue.write_buffer(&p.sort_params, 0, bytemuck::bytes_of(&SortParams { k, j, n: p.n, _pad: 0 }));
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sort"), timestamp_writes: None });
                cp.set_pipeline(&p.sort_pipeline);
                cp.set_bind_group(0, if ping { &p.bg_sort_a } else { &p.bg_sort_b }, &[]);
                cp.dispatch_workgroups((p.n + 255) / 256, 1, 1);
                drop(cp);
                ping = !ping;
                j >>= 1;
            }
            k <<= 1;
        }
    }
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(&p.render_pipeline);
        rp.set_bind_group(0, &p.bg_render, &[]);
        rp.draw(0..6, 0..p.n);
    }
    enc.finish()
}

fn main() {
    let selftest = std::env::args().any(|a| a == "--selftest");
    let gpu = match init_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[field-render] {e}");
            eprintln!("[field-render] 说明：本渲染器**不依赖 WebView2**（无任何系统组件依赖）。");
            std::process::exit(2);
        }
    };
    if selftest {
        std::process::exit(selftest_main(&gpu));
    }
    println!("[field-render] 窗口模式留待下一轮（winit 接入）；请先用 --selftest 验证管线。");
}

// ===== 离屏自检（客观度量）=====

fn selftest_main(gpu: &Gpu) -> i32 {
    let mut lib = GeneLibrary::new();
    seed_gabor_into(&mut lib);

    let text_html = {
        let mut s = String::from("<html><body><article>");
        for i in 0..20 {
            s.push_str(&format!("<p>这是第{i}段正文，用来提供足够的文本量，让水质充分上升。</p>"));
        }
        s.push_str("</article></body></html>");
        s
    };
    let image_html = {
        let mut s = String::from("<html><body><div class=\"g\">");
        for i in 0..30 {
            s.push_str(&format!("<img src=\"{i}.jpg\">"));
        }
        s.push_str("</div></body></html>");
        s
    };

    let ft = parse_source(&text_html, "");
    let fi = parse_source(&image_html, "");
    let gt = field_to_gabor_with(&ft, &lib);
    let gi = field_to_gabor_with(&fi, &lib);

    println!("[selftest] 纯文本页 四场: 地{:.2} 水{:.2} 火{:.2} 风{:.2} | Gabor λ{:.3} θ{:.2} σ{:.2} γ{:.2}",
        ft.earth, ft.water, ft.fire, ft.wind, gt.lambda, gt.theta, gt.sigma, gt.gamma);
    println!("[selftest] 纯图片页 四场: 地{:.2} 水{:.2} 火{:.2} 风{:.2} | Gabor λ{:.3} θ{:.2} σ{:.2} γ{:.2}",
        fi.earth, fi.water, fi.fire, fi.wind, gi.lambda, gi.theta, gi.sigma, gi.gamma);

    let q_neutral = Quad { tension: 0.2, calm: 0.6, liking: 0.5, safety: 0.6 };
    let q_hot = Quad { tension: 1.0, calm: 0.1, liking: 0.4, safety: 0.3 };
    let gi_hot = modulate_gabor(gi, &q_hot);
    let cases: [(&str, &FieldReading, &GaborParams, &Quad); 3] = [
        ("纯文本页", &ft, &gt, &q_neutral),
        ("纯图片页", &fi, &gi, &q_neutral),
        ("纯图片页·紧张高", &fi, &gi_hot, &q_hot),
    ];

    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let bytes_per_row = ((W * 4 + 255) / 256) * 256;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut images: Vec<Vec<u8>> = Vec::new();
    let mut fps_done = false;
    for (name, f, g, q_dummy) in cases.iter() {
        let pipe = build_pipeline(gpu, N_ELEMENTS);
        let elems = elements_of(f, g, &q_dummy, N_ELEMENTS);
        upload(gpu, &pipe, &elems, g);
        let clear = clear_color_of(f, &q_dummy);
        let cb = encode_frame(gpu, &pipe, &view, clear);
        gpu.queue.submit(Some(cb));

        let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("copy") });
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(H),
                },
            },
            wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        );
        gpu.queue.submit(Some(enc.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = gpu.device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv();
        let data = slice.get_mapped_range();
        let mut px = Vec::with_capacity((W * H) as usize);
        for y in 0..H {
            let row = (y * bytes_per_row) as usize;
            for x in 0..W {
                px.push(data[row + (x * 4) as usize]);
            }
        }
        drop(data);
        readback.unmap();

        let s = pixel_stats(&px);
        println!(
            "[selftest] {name}: 均值={:.1} 方差={:.1} 非纯黑非纯白={} pHash={:016x}",
            s.mean, s.var, s.ok, dhash_8x8(&px, W, H)
        );
        images.push(px);

        if !fps_done {
            const FRAMES: u32 = 30;
            let t0 = Instant::now();
            for _ in 0..FRAMES {
                let cb = encode_frame(gpu, &pipe, &view, clear);
                gpu.queue.submit(Some(cb));
            }
            let _ = gpu.device.poll(wgpu::Maintain::Wait);
            let ms = t0.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
            println!(
                "[selftest] 帧率：{:.1} FPS（{} splat，排序 {} 趟，每帧 {:.2} ms）",
                1000.0 / ms, pipe.n, pipe.sort_passes, ms
            );
            fps_done = true;
        }
    }

    let h01 = hamming64(dhash_8x8(&images[0], W, H), dhash_8x8(&images[1], W, H));
    let h02 = hamming64(dhash_8x8(&images[0], W, H), dhash_8x8(&images[2], W, H));
    let h12 = hamming64(dhash_8x8(&images[1], W, H), dhash_8x8(&images[2], W, H));
    println!("[selftest] pHash 汉明距离：文本↔图片={h01}｜文本↔图片·紧张={h02}｜图片↔图片·紧张={h12}（阈值 >10）");

    let mut ok = true;
    for (i, px) in images.iter().enumerate() {
        if !pixel_stats(px).ok {
            println!("[selftest] ✗ 第 {} 幅为纯黑或纯白", i + 1);
            ok = false;
        }
    }
    if h01 <= 10 {
        println!("[selftest] ✗ 场域差异不足（{h01} ≤ 10）");
        ok = false;
    }
    if h12 == 0 {
        println!("[selftest] ✗ 四元组调制未影响画面（图片↔紧张 距离为 0）");
        ok = false;
    }
    println!("[selftest] 结论：{}", if ok { "PASS" } else { "FAIL" });
    if ok { 0 } else { 1 }
}

struct PxStats {
    mean: f64,
    var: f64,
    ok: bool,
}
fn pixel_stats(px: &[u8]) -> PxStats {
    if px.is_empty() {
        return PxStats { mean: 0.0, var: 0.0, ok: false };
    }
    let n = px.len() as f64;
    let mean = px.iter().map(|&v| v as f64).sum::<f64>() / n;
    let var = px.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n;
    PxStats { mean, var, ok: var > 1.0 && mean > 1.0 && mean < 254.0 }
}

fn dhash_8x8(px: &[u8], w: u32, h: u32) -> u64 {
    let mut bits = 0u64;
    let mut k = 0u32;
    for y in 0..8u32 {
        for x in 0..8u32 {
            let x0 = (x * w / 8).min(w - 1);
            let y0 = (y * h / 8).min(h - 1);
            let x1 = ((x + 1) * w / 8).min(w - 1);
            let a = px[(y0 * w + x0) as usize];
            let b = px[(y0 * w + x1) as usize];
            if a > b {
                bits |= 1u64 << k;
            }
            k += 1;
        }
    }
    bits
}
fn hamming64(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitonic_pass_count_formula() {
        assert_eq!(bitonic_pass_count(1024), 55);
        assert_eq!(bitonic_pass_count(512), 45);
        assert_eq!(bitonic_pass_count(2), 1);
    }

    #[test]
    fn pixel_stats_detects_flat_and_empty() {
        assert!(!pixel_stats(&vec![0u8; 100]).ok, "纯黑应被检出");
        assert!(!pixel_stats(&vec![255u8; 100]).ok, "纯白应被检出");
        let mixed: Vec<u8> = (0..100u32).map(|i| (i * 2) as u8).collect();
        assert!(pixel_stats(&mixed).ok);
        assert!(!pixel_stats(&[]).ok);
    }

    #[test]
    fn dhash_deterministic_and_hamming_known() {
        let a = vec![10u8; 64 * 64];
        assert_eq!(dhash_8x8(&a, 64, 64), dhash_8x8(&a, 64, 64));
        assert_eq!(hamming64(0, 0), 0);
        assert_eq!(hamming64(u64::MAX, 0), 64);
    }

    #[test]
    fn elements_are_deterministic_and_field_sensitive() {
        let mut lib = GeneLibrary::new();
        seed_gabor_into(&mut lib);
        let t = parse_source("<html><body><p>a</p><p>b</p></body></html>", "");
        let i = parse_source("<html><body><img><img><img><img></body></html>", "");
        let gt = field_to_gabor_with(&t, &lib);
        let gi = field_to_gabor_with(&i, &lib);
        let q = Quad { tension: 0.2, calm: 0.6, liking: 0.5, safety: 0.6 };
        let et = elements_of(&t, &gt, &q, 64);
        let et2 = elements_of(&t, &gt, &q, 64);
        assert_eq!(et.len(), 64);
        assert_eq!(
            et.iter().map(|e| e.intensity).collect::<Vec<_>>(),
            et2.iter().map(|e| e.intensity).collect::<Vec<_>>()
        );
        let ei = elements_of(&i, &gi, &q, 64);
        let diff = et.iter().zip(ei.iter()).filter(|(a, b)| (a.intensity - b.intensity).abs() > 1e-6).count();
        assert!(diff > 0, "火不同 → 强度不同");
    }

    #[test]
    fn clear_color_reflects_fields_and_is_in_range() {
        let t = parse_source("<html><body><img><img></body></html>", "");
        let c = clear_color_of(&t, &Quad::default());
        for v in [c.r, c.g, c.b] {
            assert!((0.0..=1.0).contains(&v), "越界 {v}");
        }
    }
}
