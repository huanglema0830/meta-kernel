//! # 元内核 · 裸机自检与帧缓冲输出（阶段二·子任务2.2）
//!
//! **路线 A（D31-d）：零 unsafe** ——
//! 屏幕输出只经 `bootloader_api` 的**安全 API**（`FrameBuffer::buffer_mut`），
//! 不使用端口 I/O、不触碰 `unsafe`（`kernel/src` 内 `unsafe` 出现次数须为 0）。
//!
//! ## 判定协议（供 QEMU 截图机读）
//!
//! 自检结果**编码为整屏颜色**：
//! - **绿** `#00FF00` ⇒ **全部自检通过**（含内存管理门禁 + 分配往返 + 数学/L4/戒律）
//! - **黄** `#FFFF00` ⇒ **内存门禁未通过**（拿不到 `physical_memory_offset` 或没有可用物理内存区）
//!   —— 即"**拿不到堆区就停**"，**不冒充成功**
//! - **红** `#FF0000` ⇒ 其它自检失败（具体编号见 [`self_check`]／`mem::alloc_roundtrip` 的错误）
//! - **不刷屏**（保持引导器输出） ⇒ 没有可用帧缓冲，无法判定
//!
//! 这样 CI 只需断言「截图中某像素 == 绿」，即可**同时**证明：
//! ① 引导器真的把内核加载并进入了入口；② **2.1 迁出的内核子集在裸机上真的算对了**；
//! ③ **2.3 的内存管理真的能用**（门禁通过 + 经 `GlobalAlloc` 的分配/释放/复用往返成立）。

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use bootloader_api::BootInfo;
use meta_kernel_core_nostd::{fmath, l4, l4_risk, quad::Quad};

/// 通过：绿
pub const COLOR_PASS: [u8; 3] = [0x00, 0xFF, 0x00];
/// 失败：红
pub const COLOR_FAIL: [u8; 3] = [0xFF, 0x00, 0x00];
/// **内存门禁未通过**：黄 —— "拿不到堆区就停"的专用信号（与"算法算错"区分开）
pub const COLOR_NO_HEAP: [u8; 3] = [0xFF, 0xFF, 0x00];
/// ★ **诊断用**白色（画"判定编号条"）：**不参与判定**，只为让编号可被截屏读出。
pub const COLOR_DIAG: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// 判定结果（三态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 全过
    Pass,
    /// 内存门禁未过（拿不到堆区）——**不冒充成功**
    NoHeap,
    /// 其它自检失败，附编号
    Fail(u8),
}

/// 相对误差判据（f32）。
fn close(actual: f32, expect: f32, tol: f32) -> bool {
    let d = if actual > expect {
        actual - expect
    } else {
        expect - actual
    };
    d <= tol
}

/// 裸机自检。返回 `0` = 全过；非 0 = **第一个失败项的编号**（编号即失败原因，便于灰盒定位）。
///
/// 期望值一律写成**字面常量**（裸机无 `std`，不引外部数学库）：
/// √2 = 1.41421356、e = 2.71828183、ln(e) = 1、2¹⁰ = 1024。
pub fn self_check() -> u8 {
    // ① 自实现超越函数（2.1 的成果：7 个函数中的 4 个在此受检）
    if !close(fmath::sqrt(2.0), 1.414_213_5, 1e-5) {
        return 1;
    }
    if !close(fmath::exp(1.0), 2.718_281_7, 1e-5) {
        return 2;
    }
    if !close(fmath::ln(2.718_281_7), 1.0, 1e-4) {
        return 3;
    }
    if !close(fmath::powi(2.0, 10), 1024.0, 1e-2) {
        return 4;
    }

    // ② L4 戒律判据（阈值来源抽象层：由 Thresholds 注入，非直连参数库）
    let base = l4::dimension::FieldState::baseline();
    let benign = l4::dimension::FieldState::new(1.0, 1.05, 0.98, 1.0, 1.0, 1.0, 1.0);
    if l4::l4_gate::check_state(&benign, &base) != l4::l4_gate::Decision::Pass {
        return 5;
    }
    let far = l4::dimension::FieldState::new(2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);
    if l4::l4_gate::check_state(&far, &base) == l4::l4_gate::Decision::Pass {
        return 6;
    }

    // ③ 四戒律风险判定（含四元组挂接）
    let ok = l4_risk::RiskInput {
        name: "self-check",
        exposure: l4_risk::Exposure::SelfOnly,
        reversible: true,
        has_rollback: true,
        acknowledged_own_cause: true,
    };
    if l4_risk::assess(&ok, Quad::default()) != l4_risk::RiskVerdict::Cleared {
        return 7;
    }
    // 反向：触及他者 ⇒ **不害红线**，必须被拒（判据不能空转）
    let harm = l4_risk::RiskInput {
        name: "self-check",
        exposure: l4_risk::Exposure::Others,
        reversible: true,
        has_rollback: true,
        acknowledged_own_cause: true,
    };
    if l4_risk::assess(&harm, Quad::default())
        != l4_risk::RiskVerdict::Refused(l4_risk::Precept::NotHarm)
    {
        return 8;
    }

    // ④ ★ **2.3b 迁移代码在裸机上算对**（`l7::grade` —— 本轮分片第 1 片）
    //    这一段的意义：不只证明"`alloc` 能链接"，而是证明**迁移过来的真实业务代码
    //    在裸机上跑出正确结果**（含 `Vec` 路径 ⇒ 经真实 `GlobalAlloc`）。
    //    含**反向断言**（触及用户数据必须 T3 拒绝）—— 防"判据空转"。
    {
        use meta_kernel_core_nostd::l7::grade::{
            grade_of, ActionSpec, Grade, Grants, Scope, Touches,
        };

        // 正向：纯读 ⇒ T0
        let read = ActionSpec::new(1, "read-only", Scope::ReadOnly, Touches::Nothing);
        if grade_of(&read) != Grade::T0Read {
            return 9;
        }
        // 反向①：触及**用户数据** ⇒ 必须 **T3 拒绝**（红线优先于一切）
        let user_data = ActionSpec::new(2, "touch-user-data", Scope::SelfApp, Touches::UserData);
        if grade_of(&user_data) != Grade::T3Refuse {
            return 10;
        }
        // 反向②：触及**网络配置** ⇒ 必须 **T2**（**不是** T1 —— 分级不能"往下漏"）
        let net = ActionSpec::new(3, "net-config", Scope::SelfApp, Touches::NetworkConfig);
        if grade_of(&net) != Grade::T2Confirm {
            return 11;
        }
        // 反向③：不可逆且无回滚 ⇒ **T3**（"无法回滚的一律拒绝"）
        let irreversible = ActionSpec {
            id: 4,
            name: "irreversible",
            scope: Scope::SelfApp,
            touches: Touches::OwnFiles,
            reversible: false,
            has_rollback: false,
        };
        if grade_of(&irreversible) != Grade::T3Refuse {
            return 12;
        }
        // `Vec` 路径（经真实 `GlobalAlloc`）：授权**幂等**、撤销**单条**
        let mut g = Grants::new();
        g.grant(7);
        g.grant(7); // 幂等：不得重复追加
        g.grant(9);
        g.revoke(7);
        if g.granted.len() != 2 || g.granted[1] != 9 || g.revoked.len() != 1 || g.revoked[0] != 7 {
            return 13;
        }
    }

    // ⑤ ★ **2.3b 片2 迁移代码在裸机上算对**（`fourier` / `l7::mesh` / `l5_baseline`）
    //    这三条同时压到三条不同能力上：**浮点超越函数（FloatOps）**、**纯字符串解析**、
    //    **`alloc` 的 `String` + `format!` 往返**。任一条不成立 ⇒ 红屏并带编号。
    {
        use meta_kernel_core_nostd::fourier;
        use meta_kernel_core_nostd::l5_baseline::BaselineField;
        use meta_kernel_core_nostd::l7::mesh;

        // ① `fourier`：64 点、**恰好 8 个周期**的正弦 ⇒ 无泄漏 ⇒ 峰值必在 bin 8
        //    （`dft` 内部用的 `.cos()/.sin()/.sqrt()` 在 no_std 下正是 `FloatOps` 提供的）
        let mut buf = alloc::vec::Vec::with_capacity(64);
        for i in 0..64usize {
            let ph = 2.0 * core::f32::consts::PI * (i as f32) / 8.0;
            buf.push(fmath::sin(ph));
        }
        let sp = fourier::dft(&buf);
        if sp.magnitudes.len() != 33 {
            return 14; // n/2+1
        }
        if sp.dominant_bin != 8 {
            return 15;
        }
        if !close(sp.dominant_freq, 0.125, 1e-3) {
            return 16; // 8/64
        }
        if !close(sp.energy, 0.5, 1e-2) {
            return 17; // 单位正弦的均方值
        }

        // ② `l7::mesh`：版本主号解析（纯字符串、确定性）
        if mesh::major_of("1.2.3") != 1 || mesh::major_of("12.0") != 12 || mesh::major_of("bad") != 0
        {
            return 18;
        }

        // ③ `l5_baseline`：`learn` 取均值（f64）＋ `to_json`/`from_json` **往返**
        //    （`to_json` 用 `format!`/`String` ⇒ 真实走一遍 `alloc` 的 fmt 与字符串路径）
        let samples = [[1.0f64, 2.0, 3.0, 4.0], [3.0, 4.0, 5.0, 6.0]];
        let bf = BaselineField::learn(&samples, "self-check");
        if !(bf.earth > 1.999_999_999 && bf.earth < 2.000_000_001) {
            return 19;
        }
        if !(bf.wind > 4.999_999_999 && bf.wind < 5.000_000_001) {
            return 20;
        }
        let js = bf.to_json();
        match BaselineField::from_json(&js) {
            Some(rt) => {
                // ⚠️ **只比数值字段**：`from_json` 对 `object`/`established` **固定回填占位值**
                //    （`"obj"`/`"loaded"`）⇒ **不回读**。这是**原模块既有行为**（迁移逐字保留），
                //    不是本次引入；已登记为 **R27「序列化不保真」**（写进去的标识读不回来）。
                //    ⇒ 断言若拿 `rt.object == bf.object` 去比，**必然失败**（此处曾差点写错）。
                let eq = |a: f64, b: f64| a > b - 1e-9 && a < b + 1e-9;
                if !(eq(rt.earth, bf.earth) && eq(rt.water, bf.water) && eq(rt.fire, bf.fire) && eq(rt.wind, bf.wind))
                {
                    return 21; // 数值往返不一致
                }
            }
            None => return 21,
        }
    }

    // ================== ⑥ 段（2.3b 片3）：元标尺链在裸机上算得对 ==================
    //
    // 覆盖片3 的五个模块，且**每条断言都打在"这两个数必须相等/必须在界内"上**（不是"能跑就算过"）。
    // 这一段同时把三条 `no_std` 浮点替身路径**跑了一遍**：
    // f64 `round`/`sqrt`（`ontology`）、f64 `log2`（`state`）、f32 `rem_euclid`（`interference`）。
    {
        use meta_kernel_core_nostd::{energy, interference, ontology, sanitizer, state};

        // ① `sanitizer`：负值归零 + 上界钳位；**合法值不得被改动**（反向断言防"一律归零"式假过）
        if sanitizer::finalize(-1.0) != 0.0 {
            return 31;
        }
        if sanitizer::finalize(0.5) != 0.5 {
            return 31;
        }
        if sanitizer::finalize(2.0) > 1.0 {
            return 31;
        }
        if sanitizer::negative_to_zero(0.7) != 0.7 {
            return 32;
        }
        if sanitizer::negative_to_zero(-0.7) != 0.0 {
            return 32;
        }

        // ② `ontology`：常量表 + 特征向量维度与值域（内部走 f64 `abs`/`round`/`sqrt`）
        if ontology::level_name(0) != "黑" || ontology::level_name(3) != "黄" {
            return 33;
        }
        if ontology::level_name(99) != "白" {
            return 33; // 越界必须被钳到最高层（不是 panic、不是空串）
        }
        let p = ontology::Pattern::new(alloc::vec![
            ontology::Element::new(1, 0.1),
            ontology::Element::new(4, 0.5),
            ontology::Element::new(4, 0.5), // 与上一个**同层同强度** ⇒ 触发去重/重复强度路径
            ontology::Element::new(7, 0.9),
        ]);
        let s = ontology::analyze(&p);
        if s.len() != ontology::LEVELS {
            return 34;
        }
        for v in &s {
            if v.is_nan() || !(0.0..=1.0).contains(v) {
                return 34;
            }
        }
        // 反向：**结构不同的输入必须给出不同的特征向量**，否则说明算子在"空转"。
        // ⚠️ **不能拿"全黑 vs 全白"当反例** —— 实测（host 镜像测试 `tests/mirror_bare_assertions.rs`）
        //    二者输出**逐位相同**（`[1,0,0,0,0,0,1,0,0,0,0.2]`）：
        //    `analyze` 的 11 个分量是**特征轴（结构/关系）**，**不是层级振幅**，
        //    单元素模式无论挂在哪一层，这些轴上的取值都一样。
        //    ⇒ 这是我**「按函数名猜语义」**写错的第一版断言（CI 上返回 **134 = 100+34** 才暴露）。
        //    改用**元素个数/结构不同**的输入作反例（实测确有差异）。
        let p_single = ontology::Pattern::new(alloc::vec![ontology::Element::new(4, 0.5)]);
        if ontology::analyze(&p) == ontology::analyze(&p_single) {
            return 34; // 结构不同却输出相同 ⇒ 判据失效
        }

        // ③ `energy`：活力指数在界内 + 决议的**边界语义**（含 `.exp()` 路径）
        let e = energy::energy_level_evaluate(&p);
        if !(0.0..=1.0).contains(&e) || e.is_nan() {
            return 35;
        }
        if !matches!(energy::verdict_for(0.1), energy::Verdict::DecomposeToGranules) {
            return 35;
        }
        if !matches!(energy::verdict_for(0.9), energy::Verdict::Adopt) {
            return 35;
        }

        // ④ `state`：熵 → 物态（`entropy_of_history` 内部走 f64 `log2`）
        let mut hist = alloc::vec::Vec::new();
        for i in 0..8usize {
            hist.push(i as f64 / 8.0);
        }
        let ent = state::entropy_of_history(&hist);
        if ent.is_nan() || !(0.0..=1.0).contains(&ent) {
            return 36;
        }
        if state::state_of_entropy(0.9).code() != 0 {
            return 36; // ≥0.618 ⇒ 能量态
        }
        if state::state_of_entropy(0.0).code() != 3 {
            return 36; // <0.206 ⇒ 固态
        }
        // 反向：空历史按口径返回 1.0（"未分化波动"），不是 0（0 会落入固态，是错的）
        if state::entropy_of_history(&[]) != 1.0 {
            return 36;
        }

        // ⑤ `interference`：相位差与驻点检测（内部走 f32 `rem_euclid`）
        let mut wa = alloc::vec::Vec::new();
        for i in 0..32usize {
            wa.push(fmath::sin(2.0 * core::f32::consts::PI * (i as f32) / 8.0));
        }
        let d = interference::phase_difference(&wa, &wa);
        if d.is_nan() || !d.is_finite() {
            return 37;
        }
        if d.abs() > 1e-4 {
            // 同一列波与自身比 ⇒ 相位差必须为 0
            return 37;
        }
        let ps = interference::detect(&wa, &wa, 1);
        let _ = ps.len(); // 只证"能算完且不 panic"；数量语义由 host 单测覆盖
    }

    0
}

/// 按判定结果刷整屏（**安全 API**；无帧缓冲时不动屏幕，交由 CI 判定"未出绿"即失败）。
pub fn render(boot_info: &mut BootInfo, verdict: Verdict) {
    let Some(fb) = boot_info.framebuffer.as_mut() else {
        return; // 无帧缓冲：保持引导器画面（CI 会因"非绿"而红，不会误判为通过）
    };
    let info = fb.info();
    let color = match verdict {
        Verdict::Pass => COLOR_PASS,
        Verdict::NoHeap => COLOR_NO_HEAP,
        Verdict::Fail(_) => COLOR_FAIL,
    };
    let buf = fb.buffer_mut();
    fill(buf, &info, color);

    // ★ **判定编号诊断条**（2026-09-17 新增）：失败时把**编号**写成屏幕第 0 行最左 N 个**白**像素。
    // 为什么必须加：`Fail(code)` 原先只刷红屏 ⇒ **编号不可观测**，CI 只能知道"失败了"、
    // 不知道"第几号" ⇒ 每轮诊断都要重新猜（本轮 CI 首跑即撞上这个坑）。
    // 判据不受影响：像素断言只采 25 个点（y=120/240/…），**不含 y=0** ⇒ 红屏仍是红屏、门禁照样判红。
    if let Verdict::Fail(code) = verdict {
        draw_code_bar(buf, &info, code);
    }
}

/// 在第 0 行画 `code` 个白色像素（**仅用于诊断，不参与判定**）。
fn draw_code_bar(buf: &mut [u8], info: &FrameBufferInfo, code: u8) {
    let bpp = (info.bytes_per_pixel as usize).max(1);
    if info.stride < bpp {
        return;
    }
    let n = (code as usize).min(info.stride / bpp);
    let row0 = 0usize;
    for x in 0..n {
        let off = row0 + x * bpp;
        if off + bpp > buf.len() {
            break;
        }
        write_pixel(&mut buf[off..off + bpp], info.pixel_format, COLOR_DIAG);
    }
}

/// 单像素按 `PixelFormat` 写入（`p.len()` 即 `bytes_per_pixel`）。
fn write_pixel(p: &mut [u8], fmt: PixelFormat, rgb: [u8; 3]) {
    match fmt {
        PixelFormat::Rgb => {
            if p.len() >= 3 {
                p[0] = rgb[0];
                p[1] = rgb[1];
                p[2] = rgb[2];
            }
        }
        PixelFormat::Bgr => {
            if p.len() >= 3 {
                p[0] = rgb[2];
                p[1] = rgb[1];
                p[2] = rgb[0];
            }
        }
        PixelFormat::U8 => {
            if !p.is_empty() {
                p[0] = ((rgb[0] as u32 + rgb[1] as u32 + rgb[2] as u32) / 3) as u8;
            }
        }
        // 注意：PixelFormat 是 #[non_exhaustive]，兜底分支必须存在
        other => {
            if let PixelFormat::Unknown {
                red_position,
                green_position,
                blue_position,
            } = other
            {
                let v = ((rgb[0] as u32) << red_position)
                    | ((rgb[1] as u32) << green_position)
                    | ((rgb[2] as u32) << blue_position);
                for (i, byte) in p.iter_mut().enumerate() {
                    *byte = (v >> (i * 8)) as u8;
                }
            }
        }
    }
}

/// 刷满可视区域（按 `stride` 逐行推进；**边界全部显式判定**，避免越界 panic）。
fn fill(buf: &mut [u8], info: &FrameBufferInfo, rgb: [u8; 3]) {
    let bpp = info.bytes_per_pixel;
    if bpp == 0 || bpp > 8 {
        return;
    }
    let stride_bytes = info.stride.saturating_mul(bpp);
    let row_bytes = info.width.saturating_mul(bpp);
    if stride_bytes == 0 || row_bytes == 0 {
        return;
    }

    let rows = core::cmp::min(info.height, buf.len() / stride_bytes);
    for y in 0..rows {
        let base = y * stride_bytes;
        let mut col = 0usize;
        while col + bpp <= row_bytes {
            let start = base + col;
            let end = start + bpp;
            if end > buf.len() {
                break;
            }
            let (lo, hi) = (start, end);
            write_pixel(&mut buf[lo..hi], info.pixel_format, rgb);
            col += bpp;
        }
    }
}
