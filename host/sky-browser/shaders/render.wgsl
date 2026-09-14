// 场域呈现器 · Stage 3 渲染（阶段二实现；设计骨架）
struct Splat2D { position: vec2<f32>, cov2d: mat2x2<f32>, color: vec4<f32>, depth: f32 }
@group(0) @binding(0) var<storage, read> sorted_splats: array<Splat2D>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

const QUAD_CORNERS = array<vec2<f32>, 6>(
    vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
    vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0));

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, @builtin(instance_index) iid: u32) -> VertexOutput {
    let s = sorted_splats[iid];
    let corner = QUAD_CORNERS[vid % 6];
    // 2D 协方差的 Cholesky 分解 → 屏幕空间四边形
    let a = sqrt(max(s.cov2d[0][0], 1e-6));
    let b = s.cov2d[0][1] / a;
    let c = sqrt(max(s.cov2d[1][1] - b * b, 1e-6));
    let offset = vec2<f32>(a * corner.x + b * corner.y, c * corner.y) * 3.0;
    var out: VertexOutput;
    out.position = vec4<f32>(s.position + offset, s.depth, 1.0);
    out.uv = corner;
    out.color = s.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let r2 = dot(in.uv, in.uv);
    let alpha = in.color.a * exp(-2.0 * r2);
    if (alpha < 0.01) { discard; }
    return vec4<f32>(in.color.rgb * alpha, alpha);
}
