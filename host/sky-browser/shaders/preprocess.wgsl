// 场域呈现器 · Stage 1 预处理（阶段二实现；此处为设计骨架，字段与公式已与内核对齐）
// 依据：专家源头设计 §三.3；协方差投影公式 Σ' = J · W · Σ · Wᵀ · Jᵀ 为高斯泼溅标准式。
struct Camera { view_proj: mat4x4<f32>, focal: vec2<f32>, viewport: vec2<f32> }
struct GaborParams { lambda: f32, theta: f32, sigma: f32, gamma: f32, psi: f32 }
struct FieldElement { position: vec3<f32>, intensity: f32 }
struct Splat2D { position: vec2<f32>, cov2d: mat2x2<f32>, color: vec4<f32>, depth: f32 }

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> gabor: GaborParams;
@group(1) @binding(0) var<storage, read> elements: array<FieldElement>;
@group(1) @binding(1) var<storage, read_write> splats: array<Splat2D>;

// Gabor 核（与内核 gabor_wgsl() 保持同一形态；除零保护）
fn gabor_kernel(x: f32, y: f32, lambda: f32, theta: f32, psi: f32, sigma: f32, gamma: f32) -> f32 {
    let xp = x * cos(theta) + y * sin(theta);
    let yp = -x * sin(theta) + y * cos(theta);
    let gauss = exp(-(xp * xp + gamma * gamma * yp * yp) / (2.0 * sigma * sigma));
    let sinus = cos(2.0 * 3.14159265 * xp / max(lambda, 1e-4) + psi);
    return gauss * sinus;
}

// 高斯差（视网膜神经节细胞）：中心窄减周边宽
fn dog(x: f32, y: f32, sc: f32, ss: f32, b: f32) -> f32 {
    let r2 = x * x + y * y;
    let cen = exp(-r2 / (2.0 * sc * sc)) / (2.0 * 3.14159265 * sc * sc);
    let sur = exp(-r2 / (2.0 * ss * ss)) / (2.0 * 3.14159265 * ss * ss);
    return cen - b * sur;
}

@compute @workgroup_size(256)
fn preprocess(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= arrayLength(&elements)) { return; }
    let elem = elements[idx];

    let clip = camera.view_proj * vec4<f32>(elem.position, 1.0);
    if (clip.w <= 0.0) { return; }
    let ndc = clip.xyz / clip.w;
    // ⚠️ v0.116 实测修正：render.wgsl 的 `@builtin(position)` 要的是**裁剪空间**（|x|,|y| ≤ w），
    // 不是像素坐标。原骨架把像素坐标直接写进 position → 全部落在屏幕外（画面全平，方差 0）。
    // 故此处统一用 **NDC**：位置写 ndc.xy，协方差也用归一化焦距（省去 W/H 缩放）。
    let screen = ndc.xy;

    // 各向异性尺度由 Gabor 的 σ 与 γ 给出（γ 越小越"条状"）
    let sx = gabor.sigma * (1.0 - gabor.gamma * 0.5);
    let sy = gabor.sigma * (1.0 + gabor.gamma * 0.5);
    let sigma3d = mat3x3<f32>(sx * sx, 0.0, 0.0, 0.0, sy * sy, 0.0, 0.0, 0.0, 0.01);

    // 归一化 Jacobian（focal = 1，单位由「像素」改为「NDC」）——与上面的坐标口径一致
    let j = mat3x3<f32>(1.0 / clip.w, 0.0, -ndc.x / clip.w,
                        0.0, 1.0 / clip.w, -ndc.y / clip.w,
                        0.0, 0.0, 0.0);
    let cov2d = mat2x2<f32>(dot(j[0].xy, sigma3d[0].xy), dot(j[0].xy, sigma3d[1].xy),
                            dot(j[1].xy, sigma3d[0].xy), dot(j[1].xy, sigma3d[1].xy));

    // 颜色：方向 → 色相；相位 → 偏移（与内核 modulate_gabor 的 ψ 一致）
    let hue = (gabor.theta + gabor.psi) / 3.14159265;
    let color = vec4<f32>(abs(cos(hue * 6.283)), abs(cos((hue + 0.333) * 6.283)),
                          abs(cos((hue + 0.666) * 6.283)), elem.intensity);

    splats[idx].position = screen;
    splats[idx].cov2d = cov2d;
    splats[idx].color = color;
    splats[idx].depth = clip.w;
}
