// 场域呈现器 · Stage 2 排序（GPU 双调排序 bitonic；深度升序，渲染端反序读 → 远者先画）
// 说明：专家推荐 Fuchsia RadixSort 移植；本实现先落**双调排序**（同样全程在 GPU、
// 每趟一次 dispatch、n 为 2 的幂），先把闭环跑通并可客观验证；后续可平滑换成基数排序。
struct Splat2D { position: vec2<f32>, cov2d: mat2x2<f32>, color: vec4<f32>, depth: f32 }
struct SortParams { k: u32, j: u32, n: u32, _pad: u32 }

@group(0) @binding(0) var<uniform> params: SortParams;
@group(0) @binding(1) var<storage, read> src: array<Splat2D>;
@group(0) @binding(2) var<storage, read_write> dst: array<Splat2D>;

@compute @workgroup_size(256)
fn bitonic_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.n) { return; }
    let partner = i ^ params.j;
    if (partner >= params.n) { dst[i] = src[i]; return; }
    let up = ((i & params.k) == 0u);
    let a = src[i];
    let b = src[partner];
    // 升序段取小者，降序段取大者
    // 注意：naga 不允许对**结构体**用 select()（"Selecting is not possible"），必须用分支
    let take_b = select(b.depth < a.depth, b.depth > a.depth, up);
    if (take_b) {
        dst[i] = b;
    } else {
        dst[i] = a;
    }
}
