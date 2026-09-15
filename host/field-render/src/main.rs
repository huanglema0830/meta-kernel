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
mod gene_store;

use bytemuck::{Pod, Zeroable};
use std::time::Instant;

use meta_kernel_core::gene_library::GeneLibrary;
use meta_kernel_core::l1_field_parse::FieldReading;
use meta_kernel_core::l1_mapping::{
    coherence_of, field_to_gabor_with, modulate_gabor, seed_coherence_into, seed_gabor_into, GaborParams,
};
use meta_kernel_core::l3_world::WorldModel;
use meta_kernel_core::l1_source_parse::parse_source;
use meta_kernel_core::l5_attention;
use meta_kernel_core::l5_baseline::BaselineField;
use meta_kernel_core::l5_diagnosis::{self, diagnose_with_evidence};
use meta_kernel_core::l5_evidence::{self, Evidence};
use meta_kernel_core::l5_quad::{ProbeMode, Quad};
use meta_kernel_core::l5_router;

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

/// 旧签名薄包装（coherence=0）——离屏自检/诊断沿用，避免大面积改动。
fn elements_of(f: &FieldReading, g: &GaborParams, q: &Quad, n: u32) -> Vec<FieldElement> {
    elements_of_co(f, g, q, 0.0, n)
}

/// **场域元素生成（含"相位一致性"落点）**。
///
/// `coherence ∈ [0,1]` 来自「喜欢 = 预测误差降低」（内核 `l1_mapping::coherence_of`）。
/// 三处落点让画面**更清晰、更连贯**：
/// ① **抖动↓**：位置抖动按 `(1-0.85·c)` 收缩 → 排列更规则（预测误差↓）
/// ② **相位↓**（在 `upload_co` 里做）：ψ 按 `(1-0.9·c)` 收缩 → 色相不再随机跳动
/// ③ **包络↓**（在 `upload_co` 里做）：σ 按 `(1-0.25·c)` 收缩 → 更锐（更清晰）
pub fn elements_of_co(f: &FieldReading, g: &GaborParams, q: &Quad, coherence: f64, n: u32) -> Vec<FieldElement> {
    let mut out = Vec::with_capacity(n as usize);
    // 四场是 f64（内核口径），GPU 侧一律 f32 —— 此处显式收口，避免隐式转换偷偷发生
    let grid = f.earth.clamp(0.0, 1.0) as f32; // 地高 → 更接近规则网格
    let flat = f.water.clamp(0.0, 1.0) as f32; // 水大 → 更"平铺"
    let fire = f.fire.clamp(0.0, 1.0) as f32;
    let wind = f.wind.clamp(0.0, 1.0) as f32;
    let side = (n as f32).sqrt().ceil() as u32;
    let (s, c) = (g.theta as f32).sin_cos();
    let scl = 1.0 + 0.15 * g.sigma as f32;
    // 一致性：抖动收缩（越一致 → 排列越规则 → 预测误差越低）
    let coh = (coherence as f32).clamp(0.0, 1.0);
    let jitter_gain = 1.0 - 0.85 * coh;
    // 四元组 → 画面活跃度（与内核表一致：紧张 → 更活跃；平静 → 更稳定）
    let activity = ACTIVITY_BASE
        * (1.0 + 0.45 * q.tension.clamp(0.0, 1.0) as f32
            - 0.25 * q.calm.clamp(0.0, 1.0) as f32
            + 0.20 * q.liking.clamp(0.0, 1.0) as f32);
    // 强度基准均值：一致性高时把各 splat 的强度往它拉 → 画面更均匀连贯（预测误差↓）
    let mean_intensity = ((0.25 + 0.6 * fire + 0.3 * wind) * activity).clamp(0.05, 1.0);
    for i in 0..n {
        let gx = (i % side) as f32 / side as f32;
        let gy = (i / side) as f32 / side as f32;
        let h = hash01(i as u64 * 2_654_435_761 + 12_345);
        let h2 = hash01(i as u64 * 40_503 + 7_919);
        let x = (gx + (h - 0.5) * (1.0 - grid) * 0.9 * jitter_gain - 0.5) * 2.0;
        let y = (gy + (h2 - 0.5) * (1.0 - grid) * 0.9 * jitter_gain - 0.5) * 2.0;
        // z 抖动同样收缩：一致性高时深度层次更"干净"
        let z = (h - 0.5) * (1.0 - flat) * 0.5 * jitter_gain;
        let raw = ((0.25 + 0.6 * fire + 0.3 * wind) * activity).clamp(0.05, 1.0);
        // 一致性 → 强度向均值收敛（0.6·c），使画面明暗更连贯
        let intensity = (raw * (1.0 - 0.6 * coh) + mean_intensity * 0.6 * coh).clamp(0.05, 1.0);
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

/// 旧签名薄包装（coherence=0）。
fn upload(queue: &wgpu::Queue, p: &Pipeline, elems: &[FieldElement], g: &GaborParams) {
    upload_co(queue, p, elems, g, 0.0);
}

/// 上传 + **一致性对相位/包络的收缩**（见 `elements_of_co` 说明）。
pub fn upload_co(
    queue: &wgpu::Queue,
    p: &Pipeline,
    elems: &[FieldElement],
    g: &GaborParams,
    coherence: f64,
) {
    let coh = (coherence as f32).clamp(0.0, 1.0);
    let psi = (g.psi as f32) * (1.0 - 0.9 * coh); // 相位一致性：喜欢高 → 相位稳定（不跳）
    // 包络：**不随一致性变化**。实测（--like-check）：收紧 σ（缝隙变多）与展宽 σ（边缘更大更亮）
    // 都会抬高预测误差代理——该代理对**blob 边缘梯度**最敏感，σ 是它的主导项，
    // 故一致性不走 σ 这条路（否则等于在调节对比度，而非降低误差）。
    // 包络随一致性**展宽**（覆盖更连贯 → 预测残差↓）。
    // 参数扫描（--like-check，归一化预测误差的下降幅度）：
    //   σ×(1+0.0c)+jitter×(1-0.85c) → **+12.2%**（反向，不达标）
    //   σ×(1+0.8c)+jitter×(1-0.85c) → **-19.3%（达标，取此"平衡"配置）**
    //   σ×(1+1.6c)+jitter×(1-0.20c) → -37.7%（也达标，但主要靠"糊"，故不取）
    // 取平衡配置：两个机制都真实出力，而非靠单一变量把指标压下去。
    let sigma = (g.sigma as f32) * SIGMA_SCALE * (1.0 + 0.8 * coh);
    upload_inner(queue, p, elems, g, sigma, psi);
}

fn upload_inner(
    queue: &wgpu::Queue,
    p: &Pipeline,
    elems: &[FieldElement],
    g: &GaborParams,
    sigma: f32,
    psi: f32,
) {
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
            sigma, // 已含尺度换算与一致性收缩
            gamma: g.gamma as f32,
            psi,   // 已含一致性收缩
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

/// 离屏渲染目标（新验收模式共用）。
pub struct Offscreen {
    pub tex: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub rb: wgpu::Buffer,
    pub bpr: u32,
}

impl Offscreen {
    pub fn new(gpu: &Gpu, w: u32, h: u32) -> Self {
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("off"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bpr = ((w * 4 + 255) / 256) * 256;
        let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("off-rb"),
            size: (bpr * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self { tex, view, rb, bpr }
    }

    /// 渲染一帧并回读单通道（R）像素。
    pub fn render_gray(&self, gpu: &Gpu, pipe: &Pipeline, w: u32, h: u32, clear: wgpu::Color) -> Vec<u8> {
        let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("off") });
        record_field_passes(pipe, &mut enc, &self.view, clear);
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture { texture: &self.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyBuffer {
                buffer: &self.rb,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(self.bpr), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit(Some(enc.finish()));
        let slice = self.rb.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = gpu.device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv();
        let data = slice.get_mapped_range();
        let mut px = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            let row = (y * self.bpr) as usize;
            for x in 0..w {
                px.push(data[row + (x * 4) as usize]);
            }
        }
        drop(data);
        self.rb.unmap();
        px
    }
}

/// 写一个固定四元组。
fn q4(t: f64, c: f64, l: f64, s: f64) -> Quad {
    Quad { tension: t, calm: c, liking: l, safety: s }
}

/// 验收①：「喜欢」= 预测误差降低（同页面、只改 liking）→ 像素变化 >10% **且** 预测误差代理下降。
pub fn like_check_main() -> i32 {
    let gpu = match init_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[likecheck] {e}");
            return 2;
        }
    };
    let mut lib = GeneLibrary::new();
    seed_gabor_into(&mut lib);
    seed_coherence_into(&mut lib);
    let (_, image_html) = sample_pages();
    let f = parse_source(&image_html, "");
    let g_base = field_to_gabor_with(&f, &lib);
    let off = Offscreen::new(&gpu, W, H);
    let pipe = build_pipeline(&gpu.device, N_ELEMENTS, wgpu::TextureFormat::Rgba8UnormSrgb);

    let mut out: Vec<(&str, f64, Vec<u8>)> = Vec::new();
    for (name, liking) in [("喜欢=0", 0.0f64), ("喜欢=1", 1.0f64)] {
        let q = q4(0.3, 0.5, liking, 0.5); // 只改 liking
        let coh = coherence_of(&q, &lib);
        let g = modulate_gabor(g_base, &q);
        let elems = elements_of_co(&f, &g, &q, coh, N_ELEMENTS);
        upload_co(&gpu.queue, &pipe, &elems, &g, coh);
        let clear = clear_color_of(&f, &q);
        let px = off.render_gray(&gpu, &pipe, W, H, clear);
        let st = pixel_stats(&px);
        let (raw, norm) = prediction_error(&px, W, H);
        println!(
            "[likecheck] {name}: 一致性={coh:.3} 均值={:.1} 方差={:.1} 预测残差={raw:.3} **归一化预测误差={norm:.4}** 一阶梯度={:.3}(对照)",
            st.mean,
            st.var,
            neighbor_diff(&px, W, H)
        );
        out.push((name, coh, px));
    }
    let (mad, pct) = pixel_diff(&out[0].2, &out[1].2);
    let (raw0, norm0) = prediction_error(&out[0].2, W, H);
    let (raw1, norm1) = prediction_error(&out[1].2, W, H);
    let drop = if norm0 > 0.0 { (norm0 - norm1) / norm0 * 100.0 } else { 0.0 };
    println!("[likecheck] 像素变化：平均绝对差={mad:.2} 变化像素={pct:.1}%（阈值 >10%）");
    println!("[likecheck] 预测误差：残差 {raw0:.3} → {raw1:.3}｜**归一化 {norm0:.4} → {norm1:.4}（下降 {drop:.1}%，须为正）**");
    let ok = pct > 10.0 && norm1 < norm0;
    println!("[likecheck] 结论：{}", if ok { "PASS（喜欢高 → 更连贯、变化可见）" } else { "FAIL" });
    if ok { 0 } else { 1 }
}

/// 验收②：动态 LOD —— 精度随四元组变化，空闲档内存降至 1/5 以下。
pub fn lod_check_main() -> i32 {
    let gpu = match init_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[lodcheck] {e}");
            return 2;
        }
    };
    let mut lib = GeneLibrary::new();
    seed_gabor_into(&mut lib);
    seed_coherence_into(&mut lib);
    let (_, image_html) = sample_pages();
    let f = parse_source(&image_html, "");
    let g = field_to_gabor_with(&f, &lib);
    let q = q4(0.5, 0.5, 0.5, 0.5);
    let coh = coherence_of(&q, &lib);
    let elems_full = elements_of_co(&f, &g, &q, coh, 1024);
    let clear = clear_color_of(&f, &q);

    println!("[lodcheck] 四元组 → 精度档位（activity = 0.6·紧张 + 0.4·(1-平静)）");
    for (name, t_, c_) in [
        ("全平静（空闲）", 0.0, 1.0),
        ("居中", 0.5, 0.5),
        ("全紧张", 1.0, 0.0),
    ] {
        let n = lod_n_for(t_, c_);
        println!("[lodcheck]   {name}: n={n} 趟数={} 缓冲={} 字节", bitonic_pass_count(n), field_buffer_bytes(n));
    }
    let mut ok = true;
    let mut fps_by_n: Vec<(u32, f64, u64)> = Vec::new();
    for n in [1024u32, 512, 256, 128] {
        let pipe = build_pipeline(&gpu.device, n, wgpu::TextureFormat::Rgba8UnormSrgb);
        let elems = &elems_full[..n as usize];
        upload_co(&gpu.queue, &pipe, &elems, &g, coh);
        let off = Offscreen::new(&gpu, W, H);
        // 预热
        for _ in 0..5 {
            let _ = off.render_gray(&gpu, &pipe, W, H, clear);
        }
        const F: u32 = 30;
        let t0 = Instant::now();
        for _ in 0..F {
            let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("lod") });
            record_field_passes(&pipe, &mut enc, &off.view, clear);
            gpu.queue.submit(Some(enc.finish()));
        }
        let _ = gpu.device.poll(wgpu::Maintain::Wait);
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / F as f64;
        let fps = 1000.0 / ms;
        println!(
            "[lodcheck] n={n:4} 趟数={:2} 缓冲={:6} 字节 每帧={ms:.2}ms 帧率={fps:.1} FPS",
            pipe.sort_passes,
            field_buffer_bytes(n)
        );
        fps_by_n.push((n, fps, field_buffer_bytes(n)));
    }
    let (hi_n, hi_fps, hi_b) = fps_by_n[0];
    let (lo_n, lo_fps, lo_b) = *fps_by_n.last().unwrap();
    println!("[lodcheck] 空闲档 vs 满档：缓冲 {hi_b} → {lo_b} 字节（1/{:.1}）｜帧率 {hi_fps:.1} → {lo_fps:.1} FPS", hi_b as f64 / lo_b as f64);
    if !(lo_b * 5 < hi_b) {
        println!("[lodcheck] ✗ 空闲档内存未降到 1/5 以下");
        ok = false;
    }
    if lo_fps < hi_fps {
        println!("[lodcheck] ✗ 低精度档帧率反而更低（异常）");
        ok = false;
    }
    println!("[lodcheck] 精度档位随四元组单调：{}", if lod_n_for(1.0, 0.0) > lod_n_for(0.0, 1.0) { "OK" } else { "FAIL" });
    println!("[lodcheck] 结论：{}（{hi_n} → {lo_n}）", if ok { "PASS" } else { "FAIL" });
    if ok { 0 } else { 1 }
}

/// 验收③：世界模型 —— 可查询、可更新；**画面随世界模型变化**。
pub fn world_check_main() -> i32 {
    let gpu = match init_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[worldcheck] {e}");
            return 2;
        }
    };
    let mut lib = GeneLibrary::new();
    seed_gabor_into(&mut lib);
    seed_coherence_into(&mut lib);
    let (text_html, image_html) = sample_pages();
    let f_page = parse_source(&text_html, "");
    let g = field_to_gabor_with(&f_page, &lib);
    let q = q4(0.3, 0.5, 0.5, 0.5);
    let coh = coherence_of(&q, &lib);
    let pipe = build_pipeline(&gpu.device, N_ELEMENTS, wgpu::TextureFormat::Rgba8UnormSrgb);
    let off = Offscreen::new(&gpu, W, H);

    let render = |world_mean: [f64; 4], entries: usize| -> Vec<u8> {
        let fm = blend_with_world(&f_page, world_mean, 0.35, entries);
        let gm = field_to_gabor_with(&fm, &lib);
        let elems = elements_of_co(&fm, &gm, &q, coh, N_ELEMENTS);
        upload_co(&gpu.queue, &pipe, &elems, &gm, coh);
        let clear = clear_color_of(&fm, &q);
        off.render_gray(&gpu, &pipe, W, H, clear)
    };

    // ① 空世界模型
    let mut world = WorldModel::new();
    let s0 = world.summary();
    let px_empty = render(s0.mean, s0.entries);
    println!("[worldcheck] 空模型：条目={} tick={} 一致性={:.3}", s0.entries, s0.tick, s0.coherence);

    // ② 更新：观察 3 个页面 + 2 次交互
    let f_img = parse_source(&image_html, "");
    world.observe_page("example.com", "example.com/a", &f_page, 1200);
    world.observe_page("example.com", "example.com/b", &f_img, 3000);
    world.observe_page("news.cn", "news.cn/x", &f_img, 900);
    world.observe_interaction("quad", &q4(0.9, 0.1, 0.6, 0.4));
    world.observe_interaction("tab", &q4(0.2, 0.8, 0.7, 0.8));
    let s1 = world.summary();
    println!(
        "[worldcheck] 更新后：条目={} tick={} 主导={} 世界均值 地{:.2} 水{:.2} 火{:.2} 风{:.2} 一致性={:.3}",
        s1.entries, s1.tick, s1.dominant, s1.mean[0], s1.mean[1], s1.mean[2], s1.mean[3], s1.coherence
    );

    // ③ 查询：文本快照 + 往返
    let txt = world.to_text();
    let mut back = WorldModel::new();
    let loaded = back.from_text(&txt);
    println!("[worldcheck] 查询/持久化：条目 {} 条；往返载入 {} 条；一致={}", s1.entries, loaded, back.to_text() == txt);

    // ④ 画面随世界模型变化
    let px_world = render(s1.mean, s1.entries);
    let (mad, pct) = pixel_diff(&px_empty, &px_world);
    println!("[worldcheck] 画面变化（世界模型状态驱动）：平均绝对差={mad:.2} 变化像素={pct:.1}%");
    let mut ok = true;
    if s1.entries < 5 || s1.tick < 5 {
        println!("[worldcheck] ✗ 世界模型未按预期累积");
        ok = false;
    }
    if loaded != s1.entries || back.to_text() != txt {
        println!("[worldcheck] ✗ 世界模型查询/持久化往返不一致");
        ok = false;
    }
    if pct <= 10.0 {
        println!("[worldcheck] ✗ 画面未随世界模型明显变化（{pct:.1}% ≤ 10%）");
        ok = false;
    }
    println!("[worldcheck] 结论：{}", if ok { "PASS" } else { "FAIL" });
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

/// **世界模型 → 画面**：把页面场域与世界模型的累积均值合成（`alpha` 为世界权重）。
/// "画面呈现的是当前世界模型的状态"——即由这一合成体现。
pub fn blend_with_world(
    f: &FieldReading,
    mean: [f64; 4],
    alpha: f64,
    entries: usize,
) -> FieldReading {
    let a = if entries == 0 { 0.0 } else { alpha.clamp(0.0, 1.0) };
    FieldReading {
        earth: f.earth * (1.0 - a) + mean[0] * a,
        water: f.water * (1.0 - a) + mean[1] * a,
        fire: f.fire * (1.0 - a) + mean[2] * a,
        wind: f.wind * (1.0 - a) + mean[3] * a,
        confidence: f.confidence,
    }
}

/// 水平相邻像素平均绝对差（**一阶梯度**）。
///
/// ⚠️ 实测（`--like-check`）表明：在本 splat 渲染下，该量主要反映**结构/对比**（blob 边缘与整体亮度），
/// **不能**作为"预测误差"代理——收紧 σ/展宽 σ/去掉抖动都会抬高它。
/// 保留它作对照量，正式的代理见 `prediction_error`。
pub fn neighbor_diff(px: &[u8], w: u32, h: u32) -> f64 {
    if w < 2 || h == 0 || px.len() < (w * h) as usize {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for y in 0..h {
        let row = (y * w) as usize;
        for x in 1..w {
            let a = px[row + x as usize] as f64;
            let b = px[row + x as usize - 1] as f64;
            sum += (a - b).abs();
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { sum / n as f64 }
}

/// **预测误差代理**（自由能原理下的可计算量）：返回 `(残差均值, 归一化残差)`。
///
/// 定义：以**邻域线性预测**为参照，残差 `r = p[x] - (p[x-1] + p[x+1]) / 2`（即二阶差分＝局部曲率）。
/// 这正是预测编码里"用邻居预测当前像素"的误差；`|r|` 的均值越小 → 画面越可预测/越连贯。
/// `归一化残差 = 残差均值 / 对比度(std)`：**尺度无关**，避免"调亮/调对比"被误判成"误差变化"。
pub fn prediction_error(px: &[u8], w: u32, h: u32) -> (f64, f64) {
    let n_px = (w * h) as usize;
    if w < 3 || h == 0 || px.len() < n_px {
        return (0.0, 0.0);
    }
    let mean = px.iter().take(n_px).map(|&v| v as f64).sum::<f64>() / n_px as f64;
    let var = px.iter().take(n_px).map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n_px as f64;
    let std = var.sqrt();
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for y in 0..h {
        let row = (y * w) as usize;
        for x in 1..(w - 1) {
            let l = px[row + x as usize - 1] as f64;
            let c = px[row + x as usize] as f64;
            let r = px[row + x as usize + 1] as f64;
            sum += (c - (l + r) * 0.5).abs();
            n += 1;
        }
    }
    if n == 0 || std <= 1e-9 {
        return (0.0, 0.0);
    }
    let raw = sum / n as f64;
    (raw, raw / std)
}

/// 场域渲染缓冲字节数（elements 16B/splat + 两个 splat 缓冲 64B×2/splat = 144B/splat）。
/// 用于 LOD 的"内存随四元组变化"客观度量。
pub fn field_buffer_bytes(n: u32) -> u64 {
    (std::mem::size_of::<FieldElement>() as u64 + 2 * std::mem::size_of::<Splat2D>() as u64) * n as u64
}

/// **动态 LOD**：四元组 → 渲染精度档位（splat 数，取 2 的幂以适配双调排序）。
///
/// `activity = 0.6·紧张 + 0.4·(1-平静)`：紧张高 → 更多细节；平静高 → 更少细节（省资源）。
/// 返回 128 / 256 / 512 / 1024。
pub fn lod_n_for(tension: f64, calm: f64) -> u32 {
    // **单一事实源**：精度档位策略在**内核**（`l5_attention`），宿主只调用不重算。
    // 这样"渲染精度（LOD）"与"探测门槛"必然出自同一个 `activity` —— **同源才叫闭环**；
    // 宿主各存一份阈值必然出现"要高细节却同时很被动"这类自相矛盾。
    l5_attention::lod_n_for(&Quad { tension, calm, liking: 0.5, safety: 0.5 })
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



// ===== 元内核参与 =====

/// **元内核参与横幅**：每次运行都必须打印（发起人要求「每次运行请调动元内核参与」）。
///
/// 内容全部来自**内核计算**：基因库规模、诊断证据系数、注意力联动结果——
/// 一旦这些数字出现，就说明本次运行的判据取自内核，而非宿主自己又写了一套。
pub fn kernel_banner(mode: &str) {
    // **启动时加载持久化基因库**（单一事实源）：L4 阈值、证据系数等判据皆取自它。
    let mut lib = crate::gene_store::load_or_seed();
    crate::gene_store::seed_all(&mut lib);
    let s = lib.sizes();
    let gains = l5_evidence::gains_of(&lib);
    let plan = l5_attention::plan(&q4(0.5, 0.5, 0.5, 0.5), 0.0, 0.5, false);
    let th = meta_kernel_core::l4::threshold::from_library(&lib);
    println!(
        "[元内核] 参与：模式={mode}｜内核 v{}｜基因库 基础{} 场景{} 关系{} 验证{}｜L4阈高{:.4}/低{:.4}（持久化库）｜证据系数 world_gain={:.2} pe_gain={:.2} pe_ref={:.3}｜{}",
        meta_kernel_core::VERSION,
        s[0],
        s[1],
        s[2],
        s[3],
        th.high,
        th.low,
        gains.world_gain,
        gains.pe_gain,
        gains.pe_ref,
        plan.detail()
    );
}

// ===== 验收：诊断证据（世界模型匹配度 + 预测误差）=====

/// 七维样本：水（熵 H）取 0.5 → **单一水枯** → 基础确信度 0.75（**未饱和**，便于观测修正）。
///
/// 为何不用"火亢+水枯"复合样本：火偏离达 2.0 ≥ 1.618 → 基础确信度顶到 1.0，
/// 正向证据会被上界吃掉，用它测"确信度提高"会得到**假阴性**。
pub fn diag_sample() -> [f64; 7] {
    [1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0]
}

/// 验收①②：**世界模型匹配度**与**预测误差**接入 L5 诊断层。
pub fn diag_check_main() -> i32 {
    let gpu = match init_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[diagcheck] {e}");
            return 2;
        }
    };
    let mut lib = GeneLibrary::new();
    seed_gabor_into(&mut lib);
    seed_coherence_into(&mut lib);
    l5_evidence::seed_into(&mut lib);
    let gains = l5_evidence::gains_of(&lib);
    let (text_html, image_html) = sample_pages();
    let f_text = parse_source(&text_html, "");
    let f_img = parse_source(&image_html, "");
    let base = BaselineField {
        earth: 1.0,
        water: 1.0,
        fire: 1.0,
        wind: 1.0,
        object: "diagcheck",
        established: "learned",
    };
    let s = diag_sample();
    let key = "samples.local/text";
    let mut ok = true;

    // ---------- ① 世界模型匹配度 → 确信度 ----------
    println!("[diagcheck] ① 世界模型匹配度 → 诊断确信度");
    let mut world = WorldModel::new();
    for _ in 0..10 {
        world.observe_page("samples.local", key, &f_text, 2000);
    }
    // 第三个样本：**纯链接页**（大量 a、无正文、无媒体）——用于构造"真正不像"的对照
    let link_html = {
        let mut s = String::from("<html><body><nav>");
        for i in 0..300 {
            s.push_str(&format!("<a href=\"/{i}\">{i}</a>"));
        }
        s.push_str("</nav></body></html>");
        s
    };
    let f_link = parse_source(&link_html, "");
    let fld = |f: &FieldReading| format!("地{:.2} 水{:.2} 火{:.2} 风{:.2}", f.earth, f.water, f.fire, f.wind);
    println!("[diagcheck]   场域：文本页 {}｜图片页 {}｜链接页 {}", fld(&f_text), fld(&f_img), fld(&f_link));

    let m_hit = l5_evidence::match_to_world(&f_text, &world, Some(key), &gains);
    let m_miss = l5_evidence::match_to_world(&f_link, &world, Some(key), &gains);
    let m_img = l5_evidence::match_to_world(&f_img, &world, Some(key), &gains);
    println!(
        "[diagcheck]   成熟世界（同形观测 10 次 / 成熟度 {:.2}）：匹配度 {:.3}（局部 {:?}｜全局 {:.3}）",
        m_hit.maturity, m_hit.score, m_hit.local, m_hit.global
    );
    println!("[diagcheck]   异形场对同一 key：纯链接页 {:.3}｜纯图片页 {:.3}", m_miss.score, m_img.score);

    let ev_hit = Evidence { world_match: Some(m_hit.score), prediction_error: None };
    let ev_miss = Evidence { world_match: Some(m_miss.score), prediction_error: None };
    let d_none = l5_diagnosis::diagnose(&s, &base, key);
    let d_hit = diagnose_with_evidence(&s, &base, key, &ev_hit, &gains);
    let d_miss = diagnose_with_evidence(&s, &base, key, &ev_miss, &gains);
    println!(
        "[diagcheck]   确信度：无证据 {:.3}｜匹配高 {:.3}｜匹配低 {:.3}",
        d_none.conclusion.confidence, d_hit.conclusion.confidence, d_miss.conclusion.confidence
    );
    if let Some(b) = &d_hit.conclusion.basis {
        println!("[diagcheck]   依据：{}", b.note);
    }
    if !(d_hit.conclusion.confidence > d_none.conclusion.confidence) {
        println!("[diagcheck] ✗ 高匹配未提高确信度");
        ok = false;
    }
    if !(d_miss.conclusion.confidence < d_none.conclusion.confidence) {
        println!("[diagcheck] ✗ 低匹配未降低确信度");
        ok = false;
    }
    // 证据只动确信度，**不改结论**（一因一果 / 不饮酒）
    if d_hit.conclusion.title != d_miss.conclusion.title
        || d_hit.conclusion.cause != d_miss.conclusion.cause
        || d_hit.pattern != d_miss.pattern
    {
        println!("[diagcheck] ✗ 证据改动了结论本身（应当只改确信度）");
        ok = false;
    }

    // ---------- ② 预测误差 → 确信度（用**真实渲染**的误差）----------
    println!("[diagcheck] ② 预测误差 → 诊断确信度（误差取自真实渲染帧）");
    let q = q4(0.4, 0.6, 0.5, 0.6);
    let coh = coherence_of(&q, &lib);
    let g = field_to_gabor_with(&f_img, &lib);
    let pipe = build_pipeline(&gpu.device, N_ELEMENTS, wgpu::TextureFormat::Rgba8UnormSrgb);
    let off = Offscreen::new(&gpu, W, H);
    let elems = elements_of_co(&f_img, &g, &q, coh, N_ELEMENTS);
    upload_co(&gpu.queue, &pipe, &elems, &g, coh);
    let clear = clear_color_of(&f_img, &q);
    let px = off.render_gray(&gpu, &pipe, W, H, clear);
    let (_raw, pe_low) = prediction_error(&px, W, H);
    // 高误差对照：**同一把尺子上放大**（不臆造绝对量）
    let pe_high = pe_low * 8.0 + 0.20;
    println!("[diagcheck]   实测归一化预测误差 {pe_low:.4}（对照 {pe_high:.4}）");

    let e_low = diagnose_with_evidence(
        &s,
        &base,
        key,
        &Evidence { world_match: None, prediction_error: Some(pe_low) },
        &gains,
    );
    let e_high = diagnose_with_evidence(
        &s,
        &base,
        key,
        &Evidence { world_match: None, prediction_error: Some(pe_high) },
        &gains,
    );
    println!(
        "[diagcheck]   确信度：误差低 {:.3}｜误差高 {:.3}（无证据 {:.3}）",
        e_low.conclusion.confidence, e_high.conclusion.confidence, d_none.conclusion.confidence
    );
    if let Some(b) = &e_low.conclusion.basis {
        println!("[diagcheck]   依据：{}", b.note);
    }
    if !(e_low.conclusion.confidence > e_high.conclusion.confidence) {
        println!("[diagcheck] ✗ 预测误差未按预期影响确信度");
        ok = false;
    }
    if !(e_low.conclusion.confidence > d_none.conclusion.confidence) {
        println!("[diagcheck] ✗ 低误差未提高确信度");
        ok = false;
    }
    // ③ 结论**携带**预测误差原值（发起人明确要求）
    let carries = e_low
        .conclusion
        .basis
        .as_ref()
        .map(|b| b.prediction_error == Some(pe_low) && b.note.contains("预测误差"))
        .unwrap_or(false);
    println!(
        "[diagcheck] ③ 结论携带预测误差作为确信度依据：{}",
        if carries { "OK（原值 + 归一化分 + 说明）" } else { "FAIL" }
    );
    if !carries {
        ok = false;
    }

    // ④ 不成熟世界模型 → 修正被削弱（不妄语：没把握就不动结论）
    let mut young = WorldModel::new();
    young.observe_page("samples.local", key, &f_text, 2000);
    let m_young = l5_evidence::match_to_world(&f_text, &young, Some(key), &gains);
    let d_young = diagnose_with_evidence(
        &s,
        &base,
        key,
        &Evidence { world_match: Some(m_young.score), prediction_error: None },
        &gains,
    );
    let shift_young = (d_young.conclusion.confidence - d_none.conclusion.confidence).abs();
    let shift_mature = (d_hit.conclusion.confidence - d_none.conclusion.confidence).abs();
    println!(
        "[diagcheck] ④ 不成熟世界（1 次观测 / 成熟度 {:.2}）：匹配度 {:.3} → 修正量 {:.3}（成熟时 {:.3}）",
        m_young.maturity, m_young.score, shift_young, shift_mature
    );
    if !(shift_young < shift_mature) {
        println!("[diagcheck] ✗ 不成熟世界的证据影响力未被削弱");
        ok = false;
    }

    // ⑤ L5 JSON 携带依据（供 L6 呈现"为何是这个确信度"）
    let json = l5_router::to_json(&e_low);
    let json_none = l5_router::to_json(&d_none);
    let json_ok = json.contains("\"prediction_error\":") && !json.contains("\"basis\":null");
    let json_null_ok = json_none.contains("\"basis\":null");
    println!(
        "[diagcheck] ⑤ L5 JSON：有证据带依据={}｜无证据为 null={}",
        if json_ok { "OK" } else { "FAIL" },
        if json_null_ok { "OK" } else { "FAIL" }
    );
    if !(json_ok && json_null_ok) {
        ok = false;
    }

    println!("[diagcheck] 结论：{}", if ok { "PASS（世界模型匹配度与预测误差均已接入 L5 诊断层）" } else { "FAIL" });
    if ok { 0 } else { 1 }
}

// ===== 验收：LOD 与探测策略联动 =====

/// 验收③：**LOD 与探测策略联动**（注意力—渲染—探测闭环）。
///
/// 比较探测模式时必须**固定确信度与偏离**——否则测的是"感知质量"而不是"注意力"。
pub fn link_check_main() -> i32 {
    println!("[linkcheck] 注意力 → 渲染（LOD）＋ 探测（同一 activity 驱动）");
    let mut ok = true;
    const CONF: f64 = 0.40; // 中间确信度：既非"必主动"也非"必被动"，才能看出注意力带来的差别
    let dev = 0.0;

    let calm = l5_attention::plan(&q4(0.0, 1.0, 0.5, 0.5), dev, CONF, false);
    let mid = l5_attention::plan(&q4(0.5, 0.5, 0.5, 0.5), dev, CONF, false);
    let tense = l5_attention::plan(&q4(1.0, 0.0, 0.5, 0.5), dev, CONF, false);
    for (name, p) in [("全平静", calm), ("居中", mid), ("全紧张", tense)] {
        println!("[linkcheck]   {name}: {}", p.detail());
    }

    // ① 方向：紧张更细 + 更主动；平静更粗 + 更被动
    let dir_lod = tense.lod_n > calm.lod_n;
    let dir_probe = tense.probe.mode == ProbeMode::Active && calm.probe.mode == ProbeMode::Passive;
    println!(
        "[linkcheck] ① 方向：紧张 LOD {} > 平静 LOD {} = {}｜紧张{} / 平静{} = {}",
        tense.lod_n,
        calm.lod_n,
        if dir_lod { "OK" } else { "FAIL" },
        probe_str(tense.probe.mode),
        probe_str(calm.probe.mode),
        if dir_probe { "OK" } else { "FAIL" }
    );
    if !(dir_lod && dir_probe) {
        ok = false;
    }
    if tense.probe_reason.is_none() || calm.probe_reason.is_some() {
        println!("[linkcheck] ✗ 主动必须带理由、被动必须不带理由");
        ok = false;
    }

    // ② 同源：扫描 activity → 档位不降、模式不回退（不会出现"要高细节却同时被动"）
    let mut prev_n = 0u32;
    let mut saw_active = false;
    let mut monotone = true;
    for i in 0..=20 {
        let a = i as f64 / 20.0;
        let p = l5_attention::plan(&q4(a, 1.0 - a, 0.5, 0.5), dev, CONF, false);
        if p.lod_n < prev_n {
            monotone = false;
            break;
        }
        prev_n = p.lod_n;
        if p.probe.mode == ProbeMode::Active {
            saw_active = true;
        } else if saw_active {
            monotone = false;
            break;
        }
    }
    println!(
        "[linkcheck] ② 同源单调：档位不降且模式不回退 = {}｜高活动端出现过主动 = {}",
        if monotone { "OK" } else { "FAIL" },
        if saw_active { "OK" } else { "FAIL" }
    );
    if !(monotone && saw_active) {
        ok = false;
    }

    // ③ 单一事实源：宿主档位函数**逐值**等于内核
    let mut same = true;
    let mut mismatch = String::new();
    for i in 0..=20 {
        let x = i as f64 / 20.0;
        let host = lod_n_for(x, 1.0 - x);
        let kern = l5_attention::lod_n_for(&q4(x, 1.0 - x, 0.5, 0.5));
        if host != kern {
            same = false;
            mismatch = format!("x={x}: 宿主 {host} vs 内核 {kern}");
            break;
        }
    }
    println!("[linkcheck] ③ 宿主档位委托内核（逐值一致）= {}", if same { "OK" } else { "FAIL" });
    if !same {
        println!("[linkcheck]   {mismatch}");
        ok = false;
    }

    // ④ 门槛区间必须**包住旧默认门槛**：本联动不废弃旧策略，只把它的位置随注意力移动
    let lo = l5_attention::probe_floor_for_activity(0.0);
    let hi = l5_attention::probe_floor_for_activity(1.0);
    let brackets = lo < meta_kernel_core::l5_quad::PROBE_CONFIDENCE_FLOOR
        && hi > meta_kernel_core::l5_quad::PROBE_CONFIDENCE_FLOOR;
    println!(
        "[linkcheck] ④ 探测门槛区间 [{lo:.2}, {hi:.2}] 包住旧默认 {:.2} = {}",
        meta_kernel_core::l5_quad::PROBE_CONFIDENCE_FLOOR,
        if brackets { "OK" } else { "FAIL" }
    );
    if !brackets {
        ok = false;
    }

    // ⑤ 原有纪律不受影响：感知困难（确信度低/偏离大/信号陈旧）**必须**主动
    let low = l5_attention::plan(&q4(0.0, 1.0, 0.5, 0.5), 0.0, 0.05, false);
    let devp = l5_attention::plan(&q4(0.0, 1.0, 0.5, 0.5), 0.90, 0.90, false);
    let stale = l5_attention::plan(&q4(0.0, 1.0, 0.5, 0.5), 0.0, 0.90, true);
    let discipline = low.probe.mode == ProbeMode::Active
        && devp.probe.mode == ProbeMode::Active
        && stale.probe.mode == ProbeMode::Active;
    println!(
        "[linkcheck] ⑤ 原有纪律（即便全平静）：确信度低→主动={}｜偏离过大→主动={}｜信号陈旧→主动={}｜合计 {}",
        probe_str(low.probe.mode),
        probe_str(devp.probe.mode),
        probe_str(stale.probe.mode),
        if discipline { "OK" } else { "FAIL" }
    );
    if !discipline {
        ok = false;
    }

    println!("[linkcheck] 结论：{}", if ok { "PASS（LOD 与探测策略联动，同源闭环）" } else { "FAIL" });
    if ok { 0 } else { 1 }
}

// ===== 验收：宿主端到端 L4 接线（改基因库即改 L4）=====

/// 验收：**宿主端到端 L4 接线**——启动加载持久化基因库，传入 `check_state_from_library`；
/// 修改库中 L4 阈值后，宿主端 L4 判定结果随之改变，且可落盘 → 重加载保持（端到端生效）。
///
/// 用隔离的临时库，避免污染用户真实的持久化库。
pub fn l4_check_main() -> i32 {
    use meta_kernel_core::l4::dimension::FieldState;
    use meta_kernel_core::l4::l4_gate::{check_state_from_library, Decision};
    use meta_kernel_core::l4::threshold::{self, GOLDEN_HIGH, NAME_HIGH};

    let dir = std::env::temp_dir();
    let main_p = dir.join("meta-kernel-l4check-genelib.txt");
    let bak_p = dir.join("meta-kernel-l4check-genelib.bak");
    let _ = std::fs::remove_file(&main_p);
    let _ = std::fs::remove_file(&bak_p);

    // ① 加载（或播种）持久化库，并登记 L4 阈值
    let mut lib = crate::gene_store::load_or_seed_at(&main_p);
    threshold::seed_into(&mut lib);
    let base = FieldState::baseline();
    // 构造：3 维偏离 2.0（默认高判据 1.618 → 拒绝「不非时食」）
    let cur = FieldState::new(2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);

    let (d0, th0) = check_state_from_library(&cur, &base, &lib);
    println!("[l4check] ① 默认阈高={:.4}：3 维偏离 2.0 → 判定 {:?}", th0.high, d0);
    if !matches!(d0, Decision::UnseasonalMeal { .. }) {
        println!("[l4check] ✗ 默认判据下应拒绝（不非时食）");
        return 1;
    }

    // ② 改基因库：把高判据抬到 2.5 → 2.0 偏离不再越界 → 通过
    lib.set_base_constant(NAME_HIGH, 2.5, [1.0; 7]);
    let (d1, th1) = check_state_from_library(&cur, &base, &lib);
    println!("[l4check] ② 改基因库阈高={:.4}：同场域 → 判定 {:?}", th1.high, d1);
    if (th1.high - 2.5).abs() >= 1e-12 {
        println!("[l4check] ✗ 判据未从基因库读取");
        return 1;
    }
    if !matches!(d1, Decision::Pass) {
        println!("[l4check] ✗ 改基因库后判定未改变（验收项失败）");
        return 1;
    }

    // ③ 端到端：落盘 → 重新加载 → 判定仍随改后阈值（磁盘 → 内存 → 判定）
    if let Err(e) = crate::gene_store::save_gene_library_to(&lib, &main_p) {
        println!("[l4check] ✗ 落盘失败: {e}");
        return 1;
    }
    let reloaded = match crate::gene_store::load_gene_library_from(&main_p) {
        Ok(l) => l,
        Err(e) => {
            println!("[l4check] ✗ 重加载失败: {e}");
            return 1;
        }
    };
    let (d2, th2) = check_state_from_library(&cur, &base, &reloaded);
    println!("[l4check] ③ 重加载库阈高={:.4}：判定 {:?}", th2.high, d2);
    if (th2.high - 2.5).abs() >= 1e-12 || !matches!(d2, Decision::Pass) {
        println!("[l4check] ✗ 持久化后判定未保持（端到端失败）");
        return 1;
    }

    // ④ 复位：把阈高改回黄金值并落盘，避免污染后续运行
    lib.set_base_constant(NAME_HIGH, GOLDEN_HIGH, [1.0; 7]);
    let _ = crate::gene_store::save_gene_library_to(&lib, &main_p);
    let _ = std::fs::remove_file(&main_p);
    let _ = std::fs::remove_file(&bak_p);

    println!("[l4check] 结论：PASS（改基因库即改 L4，且在宿主端到端生效 + 持久化）");
    0
}

fn probe_str(m: ProbeMode) -> &'static str {
    match m {
        ProbeMode::Active => "主动",
        ProbeMode::Passive => "被动",
    }
}

/// 运行模式名（供元内核参与横幅显示）。
fn mode_name(args: &[String]) -> &'static str {
    let has = |k: &str| args.iter().any(|a| a == k);
    if has("--selftest") {
        "selftest"
    } else if has("--sortcheck") {
        "sortcheck"
    } else if has("--quad-check") {
        "quad-check"
    } else if has("--like-check") {
        "like-check"
    } else if has("--lod-check") {
        "lod-check"
    } else if has("--world-check") {
        "world-check"
    } else if has("--diag-check") {
        "diag-check"
    } else if has("--link-check") {
        "link-check"
    } else if has("--l4-check") {
        "l4-check"
    } else if has("--ui-selftest") {
        "ui-selftest"
    } else {
        "window"
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |k: &str| args.iter().any(|a| a == k);
    let val = |k: &str| -> Option<String> {
        args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
    };

    // **元内核参与**：每次运行都打印本次判据取自内核的哪一部分（发起人要求）
    kernel_banner(mode_name(&args));

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
    if has("--like-check") {
        std::process::exit(like_check_main());
    }
    if has("--lod-check") {
        std::process::exit(lod_check_main());
    }
    if has("--world-check") {
        std::process::exit(world_check_main());
    }
    if has("--diag-check") {
        std::process::exit(diag_check_main());
    }
    if has("--link-check") {
        std::process::exit(link_check_main());
    }
    if has("--l4-check") {
        std::process::exit(l4_check_main());
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
    fn lod_levels_are_powers_of_two_and_monotonic() {
        let mut prev = 0u32;
        for i in 0..=10 {
            let tension = i as f64 / 10.0;
            let n = lod_n_for(tension, 1.0 - tension);
            assert!(n.is_power_of_two(), "必须是 2 的幂：{n}");
            assert!((128..=1024).contains(&n), "越界 {n}");
            assert!(n >= prev, "紧张上升 → 精度不应下降：{prev} → {n}");
            prev = n;
        }
        assert_eq!(lod_n_for(0.0, 1.0), 128, "最平静 → 最低档");
        assert_eq!(lod_n_for(1.0, 0.0), 1024, "最紧张 → 最高档");
    }

    #[test]
    fn diag_sample_has_unsaturated_base_confidence() {
        // 样本设计的前提：基础确信度必须是 0.75（未饱和），否则正向证据会被上界吃掉，
        // 测试会变成"假阴性"——这条断言把该前提钉死，避免以后换样本时静默退化。
        let base = meta_kernel_core::l5_baseline::BaselineField {
            earth: 1.0,
            water: 1.0,
            fire: 1.0,
            wind: 1.0,
            object: "t",
            established: "learned",
        };
        let d = meta_kernel_core::l5_diagnosis::diagnose(&diag_sample(), &base, "t");
        assert!((d.conclusion.confidence - 0.75).abs() < 1e-12, "基础确信度 {}", d.conclusion.confidence);
        assert!(d.conclusion.basis.is_none(), "无证据不得产生依据");
    }

    #[test]
    fn host_lod_delegates_to_kernel() {
        // 单一事实源的守门测试：宿主档位与内核必须逐值一致
        for i in 0..=20 {
            let x = i as f64 / 20.0;
            assert_eq!(
                lod_n_for(x, 1.0 - x),
                l5_attention::lod_n_for(&Quad { tension: x, calm: 1.0 - x, liking: 0.5, safety: 0.5 }),
                "x={x} 宿主与内核档位不一致"
            );
        }
    }

    #[test]
    fn lod_memory_drops_below_one_fifth() {
        let hi = field_buffer_bytes(1024);
        let lo = field_buffer_bytes(128);
        assert_eq!(hi / lo, 8, "1024 vs 128 应为 8 倍");
        assert!(lo < hi / 5, "空闲档必须低于 1/5（实测 {lo} < {hi}/5）");
    }

    #[test]
    fn world_blend_reflects_world_state() {
        let mut lib = GeneLibrary::new();
        seed_gabor_into(&mut lib);
        let f = parse_source("<html><body><img><img><img></body></html>", "");
        let no_world = blend_with_world(&f, [0.9, 0.9, 0.9, 0.9], 0.35, 0);
        assert!((no_world.earth - f.earth).abs() < 1e-12, "无条目时不该改变画面来源");
        let with_world = blend_with_world(&f, [0.9, 0.9, 0.9, 0.9], 0.35, 5);
        assert!((with_world.earth - f.earth).abs() > 1e-6, "有世界模型时应改变");
        assert!(with_world.earth > f.earth, "世界均值更高 → 合成后更高");
    }

    #[test]
    fn coherence_reduces_prediction_error_proxy() {
        let mut lib = GeneLibrary::new();
        seed_gabor_into(&mut lib);
        let f = parse_source("<html><body><div><span x>a</span></div></body></html>", "");
        let g = field_to_gabor_with(&f, &lib);
        let q = Quad { tension: 0.3, calm: 0.5, liking: 0.5, safety: 0.5 };
        let lo = elements_of_co(&f, &g, &q, 0.0, 256);
        let hi = elements_of_co(&f, &g, &q, 1.0, 256);
        // 一致性高 → 抖动收缩 → 位置更靠近规则网格（相邻间距更均匀、方差更小）
        let spread = |v: &[FieldElement]| {
            let xs: Vec<f32> = v.iter().map(|e| e.position[0]).collect();
            let m = xs.iter().sum::<f32>() / xs.len() as f32;
            xs.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / xs.len() as f32
        };
        assert!(spread(&hi) < spread(&lo), "一致性高应更规整：{} vs {}", spread(&hi), spread(&lo));
        assert_eq!(hi.len(), lo.len());
    }

    #[test]
    fn prediction_error_is_shift_and_scale_invariant() {
        let w = 16u32;
        let h = 8u32;
        let base: Vec<u8> = (0..(w * h)).map(|i| (((i % 7) as u32 * 18) % 120 + 20) as u8).collect();
        let (_, n0) = prediction_error(&base, w, h);
        // ① 整体平移（加亮 40）：残差与 std 都对平移不变 → 归一化残差不变
        let bright: Vec<u8> = base.iter().map(|&v| v + 40).collect();
        let (_, n1) = prediction_error(&bright, w, h);
        assert!((n0 - n1).abs() < 1e-6, "平移不变：{n0} vs {n1}");
        // ② 对比度放大 1.5 倍：一阶梯度明显变大（故它衡量的是对比，不是预测误差）
        let scaled: Vec<u8> = base.iter().map(|&v| (((v as f64 - 20.0) * 1.5) + 20.0).round() as u8).collect();
        assert!(
            neighbor_diff(&scaled, w, h) > neighbor_diff(&base, w, h) * 1.3,
            "一阶梯度应随对比度上升"
        );
        // ③ 而归一化残差对缩放基本不变（含取整误差）→ 才是尺度无关的预测误差代理
        let (_, n2) = prediction_error(&scaled, w, h);
        assert!((n0 - n2).abs() < 0.05, "缩放近似不变：{n0} vs {n2}");
        // ④ 全平图：预测误差 0
        let flat = vec![100u8; (w * h) as usize];
        assert_eq!(prediction_error(&flat, w, h), (0.0, 0.0));
    }

    #[test]
    fn neighbor_diff_detects_smoothness() {
        let w = 8u32;
        let h = 4u32;
        let flat = vec![100u8; (w * h) as usize];
        assert_eq!(neighbor_diff(&flat, w, h), 0.0, "全平 → 预测误差 0");
        let mut rough = flat.clone();
        for y in 0..h {
            for x in 0..w {
                if x % 2 == 0 {
                    rough[(y * w + x) as usize] = 0;
                }
            }
        }
        assert!(neighbor_diff(&rough, w, h) > 50.0, "高频噪声 → 高预测误差");
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
