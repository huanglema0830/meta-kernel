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
//! - `field-render`（默认）：**窗口宿主**（winit 开窗 + wgpu surface + 同一套场域管线）
//!   · `--frames N`：渲染 N 帧后自动测帧率并**回读上屏像素**，然后退出（无人值守验收）
//!
//! ## 与 wry 版的关系
//! `host/sky-browser`（WebView2 版）**保留为降级路径与对照**；本 crate 完全独立。

mod window;

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

/// **必须与 WGSL 侧的 `Splat2D` 逐字节一致**（v0.121 修掉的真缺陷）。
///
/// WGSL 布局规则：`vec2<f32>` 对齐 8；`mat2x2<f32>` 对齐 8、16 字节；`vec4<f32>` **对齐 16**；`f32` 对齐 4；
/// 结构体大小向上取整到最大成员对齐。
/// 故 WGSL 实际为：position@0(8) · cov2d@8(16) · **color@32**（为满足 16 对齐，24→32）· **depth@48** · 总 **64 字节**。
///
/// 而原先的 `#[repr(C)]` 版本是 position@0 · cov2d@8 · color@**24** · depth@**40** · 总 **56 字节** ——
/// 缓冲总长按 56·n 分配，被 GPU 按 64 字节步长解读 → 数组长度只有 896（不是 1024），
/// 第 896 个之后的下标越界（WebGPU 规定越界写被丢弃、越界读返回 0）。
/// 该缺陷此前长期潜伏（此缓冲只在 GPU 内部使用，Rust 不参与），
/// 直到排序真正生效：排序会读写 `src[i ^ j]`，越界读出的 0 被写回有效槽位，逐趟把数据抹平 → **画面全平**。
#[repr(C, align(16))]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
struct Splat2D {
    position: [f32; 2],
    cov2d: [f32; 4],
    _pad0: [f32; 2],
    color: [f32; 4],
    depth: f32,
    _pad1: [f32; 3],
}
impl Default for Splat2D {
    fn default() -> Self {
        Self { position: [0.0; 2], cov2d: [0.0; 4], _pad0: [0.0; 2], color: [0.0; 4], depth: 0.0, _pad1: [0.0; 3] }
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
    /// **每趟一个 uniform**（各 16 字节）。为何不共用同一个 uniform：
    /// wgpu 的 `queue.write_buffer` 是"提交时统一落地"（`pending_writes.pre_submit` 被
    /// `active_executions.insert(0, ..)` 放在用户命令**之前**），若 55 趟共用并逐趟 `write_buffer`，
    /// 则 55 次写会全部先于 55 次 dispatch 生效 → 每趟都用最后一组参数 → **排序等于没排**。
    /// 每趟独立 uniform 后，写入互不干扰，参数与趟次一一对应。
    sort_params: Vec<wgpu::Buffer>,
    /// **每趟一个绑定组**（含该趟的 uniform 与正确的 src/dst 乒乓方向）。
    sort_bgs: Vec<wgpu::BindGroup>,
    /// 排序结果落在哪个缓冲（首趟 A→B，逐趟交替；55 趟为奇数次 → B）。render 必须读它。
    sorted_is_b: bool,
    bg_pre: wgpu::BindGroup,
    bg_io_a: wgpu::BindGroup,
    /// 渲染绑定组：读**排序结果**（正常路径）
    bg_render_sorted: wgpu::BindGroup,
    /// 渲染绑定组：读**未排序的 A**（仅诊断用，用于隔离"渲染"与"排序"哪一环出问题）
    bg_render_raw: wgpu::BindGroup,
    pre_pipeline: wgpu::ComputePipeline,
    sort_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    n: u32,
    sort_passes: u32,
}

/// 建管线（**不含 device/queue 的所有权**：由调用方持有，离屏自检与窗口宿主共用同一函数）。
///
/// `target_format` 必须与真实渲染目标一致——离屏自检传 `Rgba8UnormSrgb`，
/// 窗口宿主传 surface 的实际格式（多为 `Bgra8UnormSrgb`），否则 wgpu 会因为
/// 管线格式与 attachment 格式不匹配而报错。
fn build_pipeline(device: &wgpu::Device, n: u32, target_format: wgpu::TextureFormat) -> Pipeline {
    let d = device;

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
    // 排序趟数与"结果落在哪个缓冲"必须在建 bg_render 之前确定
    let sort_passes = bitonic_pass_count(n);
    let sorted_is_b = sort_passes % 2 == 1; // 首趟 A→B，逐趟交替
    let sorted_buf: &wgpu::Buffer = if sorted_is_b { &splats_b } else { &splats_a };
    let bg_render_sorted = d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg-render-sorted"),
        layout: &bgl_render,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: sorted_buf.as_entire_binding() }],
    });
    let bg_render_raw = d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg-render-raw"),
        layout: &bgl_render,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: splats_a.as_entire_binding() }],
    });

    // 逐趟建 uniform + 绑定组（第 m 趟：偶数 m 走 A→B，奇数 m 走 B→A）
    let mut sort_params: Vec<wgpu::Buffer> = Vec::with_capacity(sort_passes as usize);
    let mut sort_bgs: Vec<wgpu::BindGroup> = Vec::with_capacity(sort_passes as usize);
    for m in 0..sort_passes {
        let pb = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sort_params"),
            size: std::mem::size_of::<SortParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let (src, dst) = if m % 2 == 0 { (&splats_a, &splats_b) } else { (&splats_b, &splats_a) };
        sort_bgs.push(d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-sort"),
            layout: &bgl_sort,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: pb.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: dst.as_entire_binding() },
            ],
        }));
        sort_params.push(pb);
    }

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
                format: target_format,
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

    Pipeline {
        elements, splats_a, splats_b, camera_buf, gabor_buf, sort_params, sort_bgs, sorted_is_b,
        bg_pre, bg_io_a, bg_render_sorted, bg_render_raw,
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

fn upload(queue: &wgpu::Queue, p: &Pipeline, elems: &[FieldElement], g: &GaborParams) {
    let n = p.n.min(elems.len() as u32);
    queue.write_buffer(&p.elements, 0, bytemuck::cast_slice(&elems[..n as usize]));
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
    queue.write_buffer(&p.camera_buf, 0, bytemuck::bytes_of(&camera));
    // 逐趟写入各自的 uniform（互不干扰；均在本次 submit 前落地，故参数与趟次一一对应）
    let params = sort_param_series(p.n);
    debug_assert_eq!(params.len(), p.sort_params.len());
    for (m, sp) in params.iter().enumerate() {
        queue.write_buffer(&p.sort_params[m], 0, bytemuck::bytes_of(sp));
    }
    queue.write_buffer(
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

/// 生成 n 趟双调排序的 (k,j) 序列（与 `bitonic_pass_count`、着色器逐趟 dispatch 一一对应）。
pub fn sort_param_series(n: u32) -> Vec<SortParams> {
    let mut out = Vec::new();
    let mut k = 2u32;
    while k <= n {
        let mut j = k >> 1;
        while j > 0 {
            out.push(SortParams { k, j, n, _pad: 0 });
            j >>= 1;
        }
        k <<= 1;
    }
    out
}

/// 把「场域三阶段」录制进**外部 encoder**：preprocess → 双调排序 → 场域渲染。
///
/// 之所以要接收外部 encoder：UI 层必须**在同一个 encoder 内**先场域 pass、再 egui pass
/// （UI 叠在场域画面之上，共用一次提交）。
///
/// **v0.121 修正**：排序每趟的 (k,j) 曾用 `queue.write_buffer` 写同一个 uniform——
/// 而 wgpu 的 `write_buffer` 是"提交前统一落地"（`pending_writes.pre_submit` 被
/// `active_executions.insert(0, ..)` 放在用户命令**之前**），55 次写会全部先于 55 次 dispatch 生效，
/// 于是每一趟都用最后一组参数 → **排序实际没排**。现改为：数据一次性写暂存区，
/// 由 encoder 内 `copy_buffer_to_buffer` 按**命令顺序**逐趟搬到 uniform。
pub fn record_field_passes(
    p: &Pipeline,
    enc: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    clear: wgpu::Color,
) {
    record_field_passes_ex(p, enc, target, clear, true);
}

/// 同上，但可分别关闭「排序」与「读排序结果」，用于隔离诊断。
pub fn record_field_passes_ex(
    p: &Pipeline,
    enc: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    clear: wgpu::Color,
    with_sort: bool,
) {
    // Stage 1：场域元素 → 2D 高斯泼溅参数（协方差投影）
    {
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("preprocess"),
            timestamp_writes: None,
        });
        cp.set_pipeline(&p.pre_pipeline);
        cp.set_bind_group(0, &p.bg_pre, &[]);
        cp.set_bind_group(1, &p.bg_io_a, &[]);
        cp.dispatch_workgroups((p.n + 255) / 256, 1, 1);
    }
    // Stage 2：双调排序（每趟用自己的绑定组：自带该趟 uniform 与乒乓方向）
    for m in 0..if with_sort { p.sort_bgs.len() } else { 0 } {
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("sort"),
            timestamp_writes: None,
        });
        cp.set_pipeline(&p.sort_pipeline);
        cp.set_bind_group(0, &p.sort_bgs[m], &[]);
        cp.dispatch_workgroups((p.n + 255) / 256, 1, 1);
    }
    // Stage 3：渲染（屏幕空间四边形 + exp(-2r^2) 高斯衰减）
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("field-render"),
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
        rp.set_bind_group(0, if with_sort { &p.bg_render_sorted } else { &p.bg_render_raw }, &[]);
        rp.draw(0..6, 0..p.n);
    }
}

/// 只跑场域三阶段并返回可提交的命令缓冲（离屏自检用；签名保持不变）。
fn encode_frame(
    device: &wgpu::Device,
    _queue: &wgpu::Queue,
    p: &Pipeline,
    target: &wgpu::TextureView,
    clear: wgpu::Color,
) -> wgpu::CommandBuffer {
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
    record_field_passes(p, &mut enc, target, clear);
    enc.finish()
}

/// 回读**排序结果缓冲**中的 depth 序列（客观验证"排序真的排了"）。
pub fn read_sorted_depths(device: &wgpu::Device, queue: &wgpu::Queue, p: &Pipeline) -> Vec<f32> {
    let src: &wgpu::Buffer = if p.sorted_is_b { &p.splats_b } else { &p.splats_a };
    read_depths(device, queue, src, p.n)
}

/// 回读指定 spalt 缓冲的 depth 序列（`--sortcheck` 用来看排序前后对比）。
pub fn read_depths(device: &wgpu::Device, queue: &wgpu::Queue, src: &wgpu::Buffer, n: u32) -> Vec<f32> {
    let stride = std::mem::size_of::<Splat2D>();
    let size = (stride * n as usize) as u64;
    let rb = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("depth-readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("depth-copy") });
    enc.copy_buffer_to_buffer(src, 0, &rb, 0, size);
    queue.submit(Some(enc.finish()));

    let slice = rb.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    let _ = rx.recv();
    let data = slice.get_mapped_range();
    let off = std::mem::offset_of!(Splat2D, depth);
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n as usize {
        let s = i * stride + off;
        let mut b = [0u8; 4];
        b.copy_from_slice(&data[s..s + 4]);
        out.push(f32::from_le_bytes(b));
    }
    drop(data);
    rb.unmap();
    out
}

/// 回读一个 `SortParams` uniform（诊断用：确认 GPU 实际看到的是哪组 k/j/n）。
pub fn read_sort_params(device: &wgpu::Device, queue: &wgpu::Queue, buf: &wgpu::Buffer) -> SortParams {
    let size = std::mem::size_of::<SortParams>() as u64;
    let rb = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("params-readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("params-copy") });
    enc.copy_buffer_to_buffer(buf, 0, &rb, 0, size);
    queue.submit(Some(enc.finish()));
    let slice = rb.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    let _ = rx.recv();
    let data = slice.get_mapped_range();
    let out = bytemuck::pod_read_unaligned::<SortParams>(&data[..size as usize]);
    drop(data);
    rb.unmap();
    out
}

/// 逐项回读 spalt 缓冲的前 k 个元素（诊断用，仅取前 k 个）。
pub fn dump_splats(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    src: &wgpu::Buffer,
    n: u32,
    k: usize,
) -> Vec<Splat2D> {
    let stride = std::mem::size_of::<Splat2D>();
    let size = (stride * n as usize) as u64;
    let rb = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dump-readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("dump-copy") });
    enc.copy_buffer_to_buffer(src, 0, &rb, 0, size);
    queue.submit(Some(enc.finish()));
    let slice = rb.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    let _ = rx.recv();
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity(k);
    for i in 0..k.min(n as usize) {
        let s = i * stride;
        out.push(bytemuck::pod_read_unaligned::<Splat2D>(&data[s..s + stride]));
    }
    drop(data);
    rb.unmap();
    out
}

/// 客观验收：**画面随四元组变化**（同页面、同一场域，只改四元组；比 pHash 汉明距离）。
pub fn quadcheck_main() -> i32 {
    let gpu = match init_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[quadcheck] {e}");
            return 2;
        }
    };
    let mut lib = GeneLibrary::new();
    seed_gabor_into(&mut lib);
    let (_text_html, image_html) = sample_pages();
    let f = parse_source(&image_html, "");
    let g_base = field_to_gabor_with(&f, &lib);

    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("qc-offscreen"),
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
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("qc-readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let pipe = build_pipeline(&gpu.device, N_ELEMENTS, wgpu::TextureFormat::Rgba8UnormSrgb);

    let cases: [(&str, Quad); 4] = [
        ("紧张=0 / 平静=1", Quad { tension: 0.0, calm: 1.0, liking: 0.5, safety: 0.6 }),
        ("紧张=1 / 平静=0", Quad { tension: 1.0, calm: 0.0, liking: 0.5, safety: 0.6 }),
        ("喜欢=1（相位偏移）", Quad { tension: 0.2, calm: 0.6, liking: 1.0, safety: 0.6 }),
        ("安全=1（包络展宽）", Quad { tension: 0.2, calm: 0.6, liking: 0.5, safety: 1.0 }),
    ];
    let mut hashes: Vec<(String, u64, f64, Vec<u8>)> = Vec::new();
    for (name, q) in cases.iter() {
        let g = modulate_gabor(g_base, q);
        let elems = elements_of(&f, &g, q, N_ELEMENTS);
        upload(&gpu.queue, &pipe, &elems, &g);
        let clear = clear_color_of(&f, q);
        let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("qc") });
        record_field_passes(&pipe, &mut enc, &view, clear);
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyBuffer {
                buffer: &rb,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(H) },
            },
            wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        );
        gpu.queue.submit(Some(enc.finish()));
        let slice = rb.slice(..);
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
        rb.unmap();
        let st = pixel_stats(&px);
        let h = dhash_8x8(&px, W, H);
        println!("[quadcheck] {name}: 均值={:.1} 方差={:.1} pHash={:016x}", st.mean, st.var, h);
        hashes.push((name.to_string(), h, st.var, px));
    }
    let mut ok = true;
    let mut results: Vec<bool> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    println!(
        "[quadcheck] 相对「{}」的差异（同页面、只改四元组）：pHash 汉明 / 平均绝对差 / 变化像素占比",
        hashes[0].0
    );
    for (name, h, var, px) in hashes.iter().skip(1) {
        let d = hamming64(hashes[0].1, *h);
        let (mad, pct) = pixel_diff(&hashes[0].3, px);
        // 判定：像素级差异足够（同内容内的调制用像素差衡量；dHash 仅作参考）
        let pass = mad > 2.0 && pct > 5.0;
        println!(
            "[quadcheck]   {name}: 汉明={d}　平均绝对差={mad:.2}　变化像素={pct:.1}%　{}（方差 {var:.1}）",
            if pass { "PASS" } else { "FAIL" }
        );
        results.push(pass);
        if !pass {
            ok = false;
            failed.push(name.clone());
        }
    }
    let passed = results.iter().filter(|r| **r).count();
    println!(
        "[quadcheck] 结论：{}（{}/{} 个维度达到「明显变化」阈值）{}",
        if ok { "PASS" } else { "PARTIAL" },
        passed,
        results.len(),
        if ok { "".to_string() } else { format!("｜未达标：{}", failed.join("、")) }
    );
    // 退出码：全部达标=0；否则非 0（如实反映有未达标项，不掩饰）
    if ok { 0 } else { 1 }
}

/// 诊断模式：隔离「预处理 / 排序 / 渲染」哪一环出问题。
pub fn sortcheck_main() -> i32 {
    let gpu = match init_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[sortcheck] {e}");
            return 2;
        }
    };
    let mut lib = GeneLibrary::new();
    seed_gabor_into(&mut lib);
    let (_text, image_html) = sample_pages();
    let f = parse_source(&image_html, "");
    let g = field_to_gabor_with(&f, &lib);
    let q = Quad { tension: 0.2, calm: 0.6, liking: 0.5, safety: 0.6 };
    let elems = elements_of(&f, &g, &q, N_ELEMENTS);
    let pipe = build_pipeline(&gpu.device, N_ELEMENTS, wgpu::TextureFormat::Rgba8UnormSrgb);
    upload(&gpu.queue, &pipe, &elems, &g);
    let clear = clear_color_of(&f, &q);

    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sc-offscreen"),
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());

    println!("[sortcheck] n={} 趟数={} 结果在 {} 缓冲", pipe.n, pipe.sort_passes, if pipe.sorted_is_b { "B" } else { "A" });

    // 参数取证：GPU 实际看到的第 0 趟与第 54 趟参数
    let p0 = read_sort_params(&gpu.device, &gpu.queue, &pipe.sort_params[0]);
    let p_last = read_sort_params(&gpu.device, &gpu.queue, &pipe.sort_params[pipe.sort_params.len() - 1]);
    println!("[sortcheck] 参数[0]  k={} j={} n={}", p0.k, p0.j, p0.n);
    println!("[sortcheck] 参数[{}] k={} j={} n={}", pipe.sort_params.len() - 1, p_last.k, p_last.j, p_last.n);

    // A) 仅 preprocess（不排序）
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc-a") });
    record_field_passes_ex(&pipe, &mut enc, &view, clear, false);
    gpu.queue.submit(Some(enc.finish()));
    let da = read_depths(&gpu.device, &gpu.queue, &pipe.splats_a, pipe.n);
    let (ma, ua) = depth_order_report(&da);
    println!("[sortcheck] A（预处理输出）: 前 8 个 depth = {:?}", &da[..8.min(da.len())]);
    println!("[sortcheck] A: 单调非增={} 不同取值={} 首/末={:.4}/{:.4}", ma, ua, da.first().copied().unwrap_or(0.0), da.last().copied().unwrap_or(0.0));
    let ea = dump_splats(&gpu.device, &gpu.queue, &pipe.splats_a, pipe.n, 3);
    for (i, s) in ea.iter().enumerate() {
        println!("[sortcheck] A[{i}] pos=({:.3},{:.3},{:.3}) cov00={:.5} color=({:.2},{:.2},{:.2},{:.2}) depth={:.4}",
            s.position[0], s.position[1], 0.0, s.cov2d[0], s.color[0], s.color[1], s.color[2], s.color[3], s.depth);
    }

    // B) preprocess + 排序
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc-b") });
    record_field_passes_ex(&pipe, &mut enc, &view, clear, true);
    gpu.queue.submit(Some(enc.finish()));
    let db = read_depths(&gpu.device, &gpu.queue, if pipe.sorted_is_b { &pipe.splats_b } else { &pipe.splats_a }, pipe.n);
    let (mb, ub) = depth_order_report(&db);
    println!("[sortcheck] B（排序结果）: 前 8 个 depth = {:?}", &db[..8.min(db.len())]);
    println!("[sortcheck] B: 单调非增={} 不同取值={} 首/末={:.4}/{:.4}", mb, ub, db.first().copied().unwrap_or(0.0), db.last().copied().unwrap_or(0.0));
    let eb = dump_splats(&gpu.device, &gpu.queue, if pipe.sorted_is_b { &pipe.splats_b } else { &pipe.splats_a }, pipe.n, 3);
    for (i, s) in eb.iter().enumerate() {
        println!("[sortcheck] B[{i}] pos=({:.3},{:.3},*) cov00={:.5} color=({:.2},{:.2},{:.2},{:.2}) depth={:.4}",
            s.position[0], s.position[1], s.cov2d[0], s.color[0], s.color[1], s.color[2], s.color[3], s.depth);
    }
    println!("[sortcheck] 小规模逐步验证（n=8）：");
    let pipe8 = build_pipeline(&gpu.device, 8, wgpu::TextureFormat::Rgba8UnormSrgb);
    upload(&gpu.queue, &pipe8, &elems[..8], &g);
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc-8a") });
    record_field_passes_ex(&pipe8, &mut enc, &view, clear, false);
    gpu.queue.submit(Some(enc.finish()));
    let d8a = read_depths(&gpu.device, &gpu.queue, &pipe8.splats_a, 8);
    println!("[sortcheck] n=8 A = {:?}", d8a);
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc-8b") });
    record_field_passes_ex(&pipe8, &mut enc, &view, clear, true);
    gpu.queue.submit(Some(enc.finish()));
    let d8b = read_depths(&gpu.device, &gpu.queue, if pipe8.sorted_is_b { &pipe8.splats_b } else { &pipe8.splats_a }, 8);
    println!("[sortcheck] n=8 B = {:?}", d8b);
    println!("[sortcheck] n=8 降序={} 趟数={}", depth_order_report(&d8b).0, pipe8.sort_passes);
    0
}

/// 像素级差异：返回（平均绝对差, 差异超过 8 的像素占比）。
///
/// 说明：8×8 dHash 反映的是**大尺度结构**（适合"文本页 vs 图片页"这类跨内容比较）；
/// 而"同一页面、只改四元组调制"属于**同结构内的细调制**，dHash 分辨不出（实测汉明仅 3–7），
/// 必须改用像素级度量。**不同问题用不同尺子**。
pub fn pixel_diff(a: &[u8], b: &[u8]) -> (f64, f64) {
    if a.is_empty() || a.len() != b.len() {
        return (0.0, 0.0);
    }
    let n = a.len() as f64;
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64 - *y as f64).abs())
        .sum();
    let changed = a
        .iter()
        .zip(b.iter())
        .filter(|(x, y)| (**x as i32 - **y as i32).abs() > 8)
        .count() as f64;
    (sum / n, 100.0 * changed / n)
}

/// 降序检查：返回（是否单调非增, 不同取值个数）。
pub fn depth_order_report(d: &[f32]) -> (bool, usize) {
    let mono = d.windows(2).all(|w| w[0] >= w[1]);
    let mut uniq: Vec<f32> = Vec::new();
    for v in d {
        if !uniq.iter().any(|u| (u - v).abs() < 1e-6) {
            uniq.push(*v);
        }
    }
    (mono, uniq.len())
}


fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |k: &str| args.iter().any(|a| a == k);
    let val = |k: &str| -> Option<String> {
        args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
    };

    // `--selftest`：离屏渲染 + 回读 + 客观度量（无窗口，可在无显示环境/CI 跑）
    if has("--selftest") {
        let gpu = match init_gpu() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("[field-render] {e}");
                eprintln!("[field-render] 本渲染器**不依赖 WebView2**（无任何系统组件依赖）。");
                std::process::exit(2);
            }
        };
        std::process::exit(selftest_main(&gpu));
    }

    if has("--sortcheck") {
        std::process::exit(sortcheck_main());
    }
    if has("--quad-check") {
        std::process::exit(quadcheck_main());
    }

    // 默认：**窗口宿主**（winit 开窗 + wgpu surface + 同一套场域渲染管线；不依赖 WebView2）
    //   `--frames N`：渲染 N 帧后自动测帧率 + 回读上屏像素判定，然后退出（无人值守验收）
    let frames: Option<u32> = val("--frames").and_then(|v| v.parse().ok());
    let sample: usize = val("--sample").and_then(|v| v.parse().ok()).unwrap_or(0);
    let quad: Option<[f64; 4]> = val("--quad").and_then(|s| {
        let v: Vec<f64> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if v.len() == 4 {
            Some([v[0], v[1], v[2], v[3]])
        } else {
            None
        }
    });
    let url = val("--url");
    let ui_selftest = has("--ui-selftest");
    if let Err(e) = window::run(window::RunOptions { frames, sample, quad, url, ui_selftest }) {
        eprintln!("[field-render] 窗口宿主启动失败：{e}");
        std::process::exit(2);
    }
}

// ===== 共用样本（离屏自检与窗口宿主共用，保证两处"场域"定义一致）=====

/// 两个样本页面的**源码文本**：纯文本页 与 纯图片页。
pub fn sample_pages() -> (String, String) {
    let text = {
        let mut s = String::from("<html><body><article>");
        for i in 0..20 {
            s.push_str(&format!("<p>这是第{i}段正文，用来提供足够的文本量，让水质充分上升。</p>"));
        }
        s.push_str("</article></body></html>");
        s
    };
    let image = {
        let mut s = String::from("<html><body><div class=\"g\">");
        for i in 0..30 {
            s.push_str(&format!("<img src=\"{i}.jpg\">"));
        }
        s.push_str("</div></body></html>");
        s
    };
    (text, image)
}

// ===== 离屏自检（客观度量）=====

fn selftest_main(gpu: &Gpu) -> i32 {
    let mut lib = GeneLibrary::new();
    seed_gabor_into(&mut lib);

    let (text_html, image_html) = sample_pages();

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

    // 管线建一次即可（三个样本共用；也便于渲染后回读排序结果做校验）
    let pipe = build_pipeline(&gpu.device, N_ELEMENTS, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut images: Vec<Vec<u8>> = Vec::new();
    let mut fps_done = false;
    for (name, f, g, q_dummy) in cases.iter() {
        let elems = elements_of(f, g, &q_dummy, N_ELEMENTS);
        upload(&gpu.queue, &pipe, &elems, g);
        let clear = clear_color_of(f, &q_dummy);
        let cb = encode_frame(&gpu.device, &gpu.queue, &pipe, &view, clear);
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
                let cb = encode_frame(&gpu.device, &gpu.queue, &pipe, &view, clear);
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

    // 排序校验（v0.121 新增断言）：验证"远者先画"确实成立
    let depths = read_sorted_depths(&gpu.device, &gpu.queue, &pipe);
    let (mono, distinct) = depth_order_report(&depths);
    println!(
        "[selftest] 排序校验：depth {}｜不同取值 {} 个（n={}，趟数={}）",
        if mono { "已按降序排好（远者在前 → 先画）" } else { "**未按降序**（排序参数或乒乓缓冲有问题）" },
        distinct,
        pipe.n,
        pipe.sort_passes
    );

    let mut ok = true;
    if !mono {
        println!("[selftest] x 排序结果不是降序");
        ok = false;
    }
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
    fn splat2d_layout_matches_wgsl() {
        // 与 WGSL 侧 struct Splat2D 逐项对齐（见 Splat2D 注释）；这条断言就是"布局不许漂移"的守门人
        assert_eq!(std::mem::size_of::<Splat2D>(), 64, "Splat2D 必须是 64 字节（WGSL 步长）");
        assert_eq!(std::mem::offset_of!(Splat2D, position), 0);
        assert_eq!(std::mem::offset_of!(Splat2D, cov2d), 8);
        assert_eq!(std::mem::offset_of!(Splat2D, color), 32, "vec4 需 16 字节对齐");
        assert_eq!(std::mem::offset_of!(Splat2D, depth), 48);
    }

    #[test]
    fn field_element_layout_matches_wgsl() {
        // WGSL: vec3<f32> 对齐 16、占 12 字节；f32 紧随其后 → 16 字节
        assert_eq!(std::mem::size_of::<FieldElement>(), 16);
        assert_eq!(std::mem::offset_of!(FieldElement, intensity), 12);
    }

    #[test]
    fn sort_param_series_matches_pass_count_and_order() {
        for n in [2u32, 8, 64, 1024] {
            assert_eq!(sort_param_series(n).len() as u32, bitonic_pass_count(n), "n={n}");
        }
        let s = sort_param_series(1024);
        assert_eq!((s[0].k, s[0].j), (2, 1), "first pass must be (2,1)");
        let last = s.last().unwrap();
        assert_eq!((last.k, last.j), (1024, 1), "last pass must be (n,1)");
    }

    #[test]
    fn pixel_diff_metrics() {
        let a = vec![10u8; 200];
        let b = vec![10u8; 200];
        assert_eq!(pixel_diff(&a, &b), (0.0, 0.0));
        let c = vec![30u8; 200];
        let (mad, pct) = pixel_diff(&a, &c);
        assert!((mad - 20.0).abs() < 1e-9, "平均绝对差应为 20，实得 {mad}");
        assert!((pct - 100.0).abs() < 1e-9);
        assert_eq!(pixel_diff(&[], &a), (0.0, 0.0), "长度不符时返回 0");
    }

    #[test]
    fn depth_order_report_detects_descending() {
        assert_eq!(depth_order_report(&[3.0, 2.0, 1.0]), (true, 3));
        assert!(!depth_order_report(&[1.0, 2.0, 3.0]).0, "ascending is the wrong direction");
        assert_eq!(depth_order_report(&[0.5, 0.5, 0.5]), (true, 1), "all equal means identity sort");
        assert_eq!(depth_order_report(&[]), (true, 0));
    }

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
