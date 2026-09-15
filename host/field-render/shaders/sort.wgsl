// 场域呈现器 · Stage 2 排序（GPU 双调排序 bitonic）
// **方向：按 depth 降序**（depth 大 = 远 → 排在前面 → 先画 → 后画的近者覆盖在上）
// 修正记录（v0.121）：原先排「升序」并注释说"渲染端反序读"，但渲染端实际是**正序**遍历实例，
// 于是最近的先画、最远的最后画 → 叠加顺序反了。改为直接降序排序，使"远者先画"名副其实。
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
    // ---- 比较交换方向（v0.121 修掉的真缺陷）----
    // 双调网络里，**一对 (i, partner) 的两个线程必须取相反的极值**：
    //   低位者（(i & j) == 0）与高位者（(i & j) != 0）一个拿 min、一个拿 max。
    // 原实现只判 `i & k`（块方向）而对 i 与 partner 用**同一条规则**，
    // 两者 `i & j` 不同却都取了 max → 每趟都丢掉极小值，数据逐趟塌缩，
    // 最终整段变成同一个值（实测 n=8 与 n=1024 都退化为"全等于 A[0]"的广播）。
    let up = ((i & params.k) == 0u); // 本块方向：因整体要做**降序**，故 up 块＝降序块
    let lo = ((i & params.j) == 0u); // 在本比较对中位于低位
    let want_min = (up != lo);       // 低位/高位取相反极值

    let a = src[i];
    let b = src[partner];
    // 注意：naga 不允许对**结构体**用 select()（"Selecting is not possible"），必须用分支。
    // 相等时保持原位（`<` / `>` 都为假 → 取 a），保证结果仍是**排列**而非复制。
    var take_b: bool;
    if (want_min) {
        take_b = b.depth < a.depth;
    } else {
        take_b = b.depth > a.depth;
    }
    if (take_b) {
        dst[i] = b;
    } else {
        dst[i] = a;
    }
}
