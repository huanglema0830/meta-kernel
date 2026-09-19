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

    // —— ⑦ 段：2.3b 片4（10 模块：痕迹／基因库／场域解析／世界模型／四元组／语境／证据／习气）——
    let r4 = self_check_shard4();
    if r4 != 0 {
        return r4; // 41–45（经 main 的 `100 + n` 映射后，CI 上会读成 141–145）
    }

    // —— ⑧ 段：2.3b 片5（16 模块：源解析／思维链／沙漏／演化／注意力／闸门／自识别／账本…）——
    let r5 = self_check_shard5();
    if r5 != 0 {
        return r5; // 51–55（经 main 的 `100 + n` 映射后，CI 上会读成 151–155）
    }

    // —— ⑨ 段：2.3b 片6（4 模块：L1 视觉映射／L5 诊断／L7 修复建议／正源解构）——
    let r6 = self_check_shard6();
    if r6 != 0 {
        return r6; // 91–95（经 main 的 `100 + n` 映射后，CI 上会读成 191–195）
    }

    // —— ⑩ 段：2.3b 片7／片8（3 模块：L5 翻译／L6 脸切换／L5 JSON 路由）——
    let r7 = self_check_shard7();
    if r7 != 0 {
        return r7; // 101–105（经 main 的 `100 + n` 映射后，CI 上会读成 201–205）
    }

    // —— ⑪ 段：Q11 物态→引擎选择（`engine_select`：选择表／1.05 不切换／预算封顶／双面一致／不调制）——
    let r8 = self_check_q11_engine_select();
    if r8 != 0 {
        return r8; // 111–115（经 main 的 `100 + n` 映射后，CI 上会读成 211–215）
    }

    0
}

// ================== ⑨ 段（2.3b 片6）：孪生配对 / 诊断中性点 / 动作白名单 ==================
//
// **为什么选这三个**：它们各自是**可证伪的契约**，且**恰好覆盖本片的两处「替换类」**：
//   * 孪生配对（`positive_source`）：`twin_index` 是本片**唯一的类型替换**落点
//     （`HashMap` → `BTreeMap`）。**幂等 + 用孪生键查得回**这两条断言，直接证成
//     「换容器后 `get`/`insert` 语义不变」——即**行为等价**（不是"编译过了就算"）。
//   * 诊断中性点（`l5_diagnosis`）：**R7 教训** —— 中性点必须 "输入=本底 ⇒ 结论=平"，
//     否则任何输入都会被报成异常（判据空转）。
//   * 动作白名单（`l7::repair`）：**找不到即 None ⇒ 宿主必须拒绝** —— 反向断言证明
//     "白名单之外的 id 不可执行"，这是 L7「不侵」在编译期目录上的落点。
#[allow(clippy::too_many_lines)]
fn self_check_shard6() -> u8 {
    use meta_kernel_core_nostd::{l5_compare::Band, l5_diagnosis, l1_mapping, l7, positive_source};
    use meta_kernel_core_nostd::l5_baseline::BaselineField;

    // ——— 91：孪生指纹可逆（`twin(twin(x)) == x`），且永不恒等 ———
    {
        for x in [0u64, 1, 0xDEAD_BEEF_u64, u64::MAX, 0x8000_0000_0000_0000] {
            if positive_source::twin_fingerprint(positive_source::twin_fingerprint(x)) != x {
                return 91; // 可逆性破 ⇒ "瞬时可逆"是假话
            }
            if positive_source::twin_fingerprint(x) == x {
                return 91; // 恒等 ⇒ 孪生与本体重合，配对无意义
            }
        }
    }

    // ——— 92/93：孪生索引往返（**直接证 `BTreeMap` 换容器行为等价**）———
    {
        let mut ps = positive_source::PositiveSource::new();
        let fp: u64 = 0x0123_4567_89AB_CDEF;
        ps.entangle(fp, 0.4);
        if ps.entangled_len() != 1 {
            return 92;
        }
        // 用「孪生键」查得回（走 `twin_index.get`）
        if ps.entanglement_match(positive_source::twin_fingerprint(fp)) != Some(0.4) {
            return 92; // 插入后查不回 ⇒ 索引失效
        }
        // 幂等：同正指纹再登记 ⇒ **条目数不变**（走 `twin_index.get` 命中分支），但补充增量被更新
        ps.entangle(fp, 0.9);
        if ps.entangled_len() != 1 {
            return 93; // 幂等破 ⇒ `get` 命中逻辑失效（换容器最可能伤到这里）
        }
        if ps.entanglement_match(positive_source::twin_fingerprint(fp)) != Some(0.9) {
            return 93; // 更新未生效
        }
        // 反向：**未登记**的孪生键必须查不到（否则"配对"退化成"永远命中"）
        if ps.entanglement_match(fp) != None {
            return 93;
        }
    }

    // ——— 94：诊断中性点（输入 = 本底 ⇒ 全场平 ⇒ `advice.calm`；R7 教训）———
    {
        let base = BaselineField {
            earth: 0.6,
            water: 0.6,
            fire: 0.6,
            wind: 0.6,
            object: "self-check",
            established: "self-check",
        };
        let c = l5_diagnosis::synthesize(&[0.6; 4], &base, &[Band::Ping; 4]);
        // `suggestion_key` 是语言无关键（`String`）——**能取到非空值本身就证成 `format!`/`String` 在 no_std 下可用**
        if c.suggestion_key != "advice.calm" {
            return 94; // 中性点未映射到"维持现状" ⇒ 任何输入都会被报成异常
        }
        if c.suggestion.is_empty() {
            return 94; // 默认文本缺失（`String` 路径未真正工作）
        }
    }

    // ——— 95：动作白名单（在册 id 可取；**不在册必 None**）———
    {
        for id in 1u32..=4 {
            match l7::repair::action_by_id(id) {
                Some(a) if a.id == id => {}
                _ => return 95,
            }
        }
        for id in [0u32, 5, 99, u32::MAX] {
            if l7::repair::action_by_id(id).is_some() {
                return 95; // 白名单之外的 id 竟能取到 ⇒「不侵」的编译期目录失效
            }
        }
        if l7::repair::action_by_key("clean-temp").is_none() {
            return 95; // 稳定键查不到 ⇒ 宿主无法对表执行
        }
    }

    // ——— 附：L1 视觉映射可算且值域合法（片6 第四模块的最小存在性断言）———
    {
        let g = l1_mapping::GaborParams::default();
        if !(g.lambda.is_finite() && g.theta.is_finite() && g.sigma.is_finite() && g.gamma.is_finite()) {
            return 91;
        }
        if !l1_mapping::GABOR_DEFAULTS.iter().all(|v| v.is_finite()) {
            return 91;
        }
    }

    0
}

// ====== ⑩ 段（2.3b 片7／片8）：翻译回退 / 脸叠层上界 / JSON 结构配平 ======
//
// **为什么选这三个**：它们是本片三模块各自**可证伪的契约**，且各压住一条**底图条文**：
//   * 翻译回退（`l5_translate`）：底图 `L5_TRANSLATION_TABLE` 明定「缺词／未验证 ⇒ 保留场语言原文
//     ＋ `(待补)` 标记，**不阻断诊断**」⇒ 用**空词表**跑，断言输出**必含 `(待补)`**
//     （若不含，说明缺词被静默吞掉 ⇒「不妄语」破）。
//   * 脸叠层（`l6_face`）：底图 `L6_DESIGN`／`FIELD_PRESENTATION_DESIGN` 定「**网页始终可见**」⇒
//     断言 ① 原版 α 恒 0；② **一切模式 × 一切置信度 α ≤ 0.35**；③ 混合恰为场域之半。
//   * JSON 路由（`l5_router`）：**手写**序列化 ⇒ 断言**结构首尾 ＋ 引号/反斜杠计数配平**
//     （转义写坏则配平必破）——这是「零依赖 JSON」在 no_std 下**真正可用**的最小证据。
#[allow(clippy::too_many_lines)]
fn self_check_shard7() -> u8 {
    use meta_kernel_core_nostd::l5_baseline::BaselineField;
    use meta_kernel_core_nostd::l5_diagnosis::{diagnose, Diagnosis};
    use meta_kernel_core_nostd::{l5_router, l5_translate, l6_face};

    let base = BaselineField {
        earth: 0.6,
        water: 0.6,
        fire: 0.6,
        wind: 0.6,
        object: "self-check",
        established: "self-check",
    };
    // 诊断输入构造（与 `l5_router` 单测同形）；**实测 pattern = Kang,Ping,Ping,Ping**
    // （**不要假设它全为 Ping** —— 该假设在 host 镜像上已被实测否定）
    let d: Diagnosis = diagnose(&[0.6, 0.6, 0.6, 0.6, 1.0, 0.5, 1.0], &base, "shard78");

    // ——— 101：缺词必须如实标记 `(待补)`，且**不阻断**（空词表仍能跑出结果）———
    {
        let empty: [l5_translate::Term; 0] = [];
        let s = l5_translate::summarize_for(&empty, "universal", &d);
        if !s.contains("(待补)") {
            return 101; // 缺词被静默吞掉 ⇒ 「不妄语」破
        }
    }

    // ——— 102：8 语言齐备且逐条非空（证 `Vec<(String,String)>` 在 no_std 下真可用）———
    {
        let all = l5_translate::all_summaries(&d);
        if all.len() != l5_translate::LANGS.len() || all.len() != 8 {
            return 102;
        }
        for (k, v) in all.iter() {
            if k.is_empty() || v.is_empty() {
                return 102;
            }
        }
    }

    // ——— 103：原版永不叠加；且**一切模式 α ≤ 0.35**（「网页始终可见」的量化上界）———
    {
        use l6_face::FaceMode;
        if FaceMode::Original.overlay_alpha(1.0) != 0.0 {
            return 103;
        }
        for m in FaceMode::all() {
            for c in [0.0f64, 0.5, 1.0] {
                let a = m.overlay_alpha(c);
                if !(0.0..=0.35).contains(&a) {
                    return 103; // 上界破 ⇒ 叠层可能遮住内容
                }
            }
        }
    }

    // ——— 104：混合恰为场域之半；未知输入走**安全缺省**（不叠加）———
    {
        use l6_face::FaceMode;
        let f = FaceMode::Field.overlay_alpha(1.0);
        let b = FaceMode::Blend.overlay_alpha(1.0);
        let diff = b * 2.0 - f;
        if !(diff < 1e-12 && -diff < 1e-12) {
            return 104; // 「减半」名不符实
        }
        if FaceMode::parse("乱码") != FaceMode::Original {
            return 104; // 未知输入未走安全缺省 ⇒ 可能误叠加
        }
    }

    // ——— 105：JSON 结构首尾 ＋ 引号/反斜杠配平（转义写坏必被抓住）———
    {
        let j = l5_router::to_json(&d);
        if !(j.starts_with('{') && j.ends_with('}')) {
            return 105;
        }
        if !j.contains("\"schema\":") || !j.contains("\"universal\":") {
            return 105;
        }
        if j.matches('"').count() % 2 != 0 || j.matches('\\').count() % 2 != 0 {
            return 105; // 配平破 ⇒ 转义不成立
        }
    }

    0
}

// ====== ⑪ 段（Q11）：物态→引擎选择 —— 选择表 / 1.05 不切换 / 预算封顶 / 双面一致 / 不调制 ======
//
// **为什么选这五条**：它们是 Q11 可测契约的**可证伪面**，每条各压住一条**裁定**：
//   * **选择表**（`engine_for_state`）：Q8 裁定「固态→线性／液态→斐波那契／气态→斐波那契／能量态→指数」。
//     纯映射、无浮点 ⇒ 期望值**唯一可判**（不"能算完就行"）。
//   * **1.05 不切换**：`GENE_LIBRARY_DESIGN §2.1 行61` 用**严格不等式** ＋ Q10 **丙案**（阈值处不切换）
//     ⇒ **实际只有 2 个引擎切换点（0.8／1.2）**。与 V2 实测「`1.05` 处差值 0」吻合。
//   * **预算封顶**：U1 裁定＝**乙**（物态取 `state_of_energy_budget`）⇒ **储备枯竭必须把物态拉向更固者**，
//     即使瞬时比值偏高。**反向**：储备充足时不得无故降级。
//   * **双面一致**：U2 裁定＝**丙**（标签面 ＋ 结果面各出一个可测面）⇒ 两面必须**自洽**
//     （把"选择"与"结果"对不上的实现抓出来）。
//   * **不调制**：Q10 前问＝**选择器**（不是调制量）⇒ 输入是原种子 A1 原样；
//     故"**选同一引擎的两个不同比值池**"的结果面必须**逐位相同**
//     （若实现把 `r` 混进输入 ⇒ 此断言必破）。
#[allow(clippy::too_many_lines)]
fn self_check_q11_engine_select() -> u8 {
    use meta_kernel_core_nostd::energy::EnergyPool;
    use meta_kernel_core_nostd::engine_select::{
        engine_for_state, select_and_step, select_engine, step_with, Engine,
    };
    use meta_kernel_core_nostd::state::{state_of_flow_ratio, State};

    /// ε 口径（R35）：`f32` 在 `1.05` 附近 ULP ≈ `1.19e-7`；取 `1e-6`（可表示、显著大于 ULP）。
    const EPS: f32 = 1e-6;

    // ——— 111：选择表（Q8）四档唯一映射 ———
    {
        let cases = [
            (0.4_f32, State::Solid, Engine::Linear),
            (0.9, State::Liquid, Engine::Fibonacci),
            (1.1, State::Gas, Engine::Fibonacci),
            (1.5, State::Energy, Engine::Expo),
        ];
        for (r, want_state, want_eng) in cases {
            let s = state_of_flow_ratio(r);
            if s != want_state {
                return 111; // 物态错 ⇒ 阈值/不等号写错
            }
            if engine_for_state(s) != want_eng {
                return 111; // 映射错 ⇒ Q8 选择表被改坏
            }
        }
    }

    // ——— 112：1.05 处「物态切换、引擎不切换」；0.8／1.2 才是真切换点 ———
    {
        let s_lo = state_of_flow_ratio(1.05 - EPS);
        let s_hi = state_of_flow_ratio(1.05 + EPS);
        if s_lo == s_hi {
            return 112; // 物态应切换（严格不等式）
        }
        if engine_for_state(s_lo) != engine_for_state(s_hi) {
            return 112; // 引擎**不得**切换（丙案核心）
        }
        if engine_for_state(s_lo) != Engine::Fibonacci {
            return 112; // 液态／气态应共用斐波那契
        }
        if engine_for_state(state_of_flow_ratio(0.8 - EPS))
            == engine_for_state(state_of_flow_ratio(0.8 + EPS))
        {
            return 112; // 0.8 应是真切换点
        }
        if engine_for_state(state_of_flow_ratio(1.2 - EPS))
            == engine_for_state(state_of_flow_ratio(1.2 + EPS))
        {
            return 112; // 1.2 应是真切换点
        }
    }

    // ——— 113：U1＝乙 · 预算封顶（储备枯竭拉向更固者；充足时不无故降级）———
    {
        // 比值态 = Energy（flow_out=0 ⇒ ratio 落上界 9.0），但储备枯竭 ⇒ 预算态 = Solid
        let starved = EnergyPool { flow_in: 1.0, flow_out: 0.0, stored: 0.0 };
        if select_engine(&starved) != Engine::Linear {
            return 113; // 未被预算封顶 ⇒ U1 的"取更固者"失效
        }
        // 储备充足 ⇒ 不受约束，回到 Expo
        let rich = EnergyPool { flow_in: 1.0, flow_out: 0.0, stored: 1.0 };
        if select_engine(&rich) != Engine::Expo {
            return 113; // 无故降级 ⇒ 预算约束写反
        }
    }

    // ——— 114：U2＝丙 · 双面一致（选择面 ↔ 结果面）＋ 结果面可复现 ———
    {
        let pools = [
            EnergyPool { flow_in: 0.4, flow_out: 1.0, stored: 1.0 },  // Solid → Linear
            EnergyPool { flow_in: 0.9, flow_out: 1.0, stored: 1.0 },  // Liquid → Fibonacci
            EnergyPool { flow_in: 1.15, flow_out: 1.0, stored: 1.0 }, // Gas → Fibonacci
            EnergyPool { flow_in: 1.0, flow_out: 0.0, stored: 1.0 },  // Energy → Expo
        ];
        for p in pools {
            let (e, y) = select_and_step(&p, 0.5);
            if e != select_engine(&p) {
                return 114; // 标签面与选择不一致
            }
            if y.to_bits() != step_with(e, 0.5).to_bits() {
                return 114; // 结果面与"按标签走一步"不一致 ⇒ 双面不自洽
            }
            if y.to_bits() != select_and_step(&p, 0.5).1.to_bits() {
                return 114; // 不可复现 ⇒ 跨调用累积了状态
            }
        }
    }

    // ——— 115：Q10 前问 · 选择器**不调制输入**（同一引擎、不同比值 ⇒ 结果面逐位相同）———
    {
        // 两个池：比值分属「液态」(≈0.90) 与「气态」(≈1.15)，**引擎同为斐波那契**
        let liquid = EnergyPool { flow_in: 0.9, flow_out: 1.0, stored: 1.0 };
        let gas = EnergyPool { flow_in: 1.15, flow_out: 1.0, stored: 1.0 };
        let e_liq = select_engine(&liquid);
        let e_gas = select_engine(&gas);
        if e_liq != e_gas || e_liq != Engine::Fibonacci {
            return 115; // 前提不成立（两池未同选斐波那契）
        }
        if step_with(e_liq, 0.5).to_bits() != step_with(e_gas, 0.5).to_bits() {
            return 115; // 同引擎结果面不同 ⇒ 输入被调制（Q10 前问破）
        }
        // 1.05 两侧同引擎 ⇒ 结果面亦须逐位相同（对应 V2 实测"1.05 处差值 0"）
        let lo = select_engine(&EnergyPool { flow_in: 0.95, flow_out: 1.0, stored: 1.0 });
        let hi = select_engine(&EnergyPool { flow_in: 1.10, flow_out: 1.0, stored: 1.0 });
        if lo != hi {
            return 115; // 0.95(Liquid) 与 1.10(Gas) 应同引擎
        }
    }

    0
}

// ================== ⑧ 段（2.3b 片5）：三态判定 / 动作账链 / 沙漏瓶颈 ==================
//
// **为什么选这三个**：它们各自是**可证伪的契约**，而不是"能算完就行"：
//   * 三态判定（`l5_compare`）：`dev=|cur/base|` 与黄金阈值比较 ⇒ **三态必须都能被取到**，
//     否则判据在空转（任何输入都同一结论）。
//   * 动作账（`l7::ledger`）：链自洽 + **篡改必拒**（反向断言）⇒ 证明"校验"不是摆设。
//   * 沙漏（`hourglass`）：每 tick **至多放行 1 粒** ⇒ 容量/节流语义为真。
fn self_check_shard5() -> u8 {
    use meta_kernel_core_nostd::{hourglass, l5_baseline::BaselineField, l5_compare, l7};

    // ——— 51：三态判定（本底自比必平；2× 必亢；0.5× 必枯）———
    {
        let base = BaselineField {
            earth: 0.7,
            water: 0.7,
            fire: 0.7,
            wind: 0.7,
            object: "self-check",
            established: "self-check",
        };
        if !l5_compare::compare(&[0.7; 4], &base).iter().all(|b| *b == l5_compare::Band::Ping) {
            return 51; // 本底自比必须全平（契约）
        }
        if !l5_compare::compare(&[1.4; 4], &base).iter().all(|b| *b == l5_compare::Band::Kang) {
            return 51; // 2.0 倍 > 1.618 ⇒ 全亢
        }
        if !l5_compare::compare(&[0.35; 4], &base).iter().all(|b| *b == l5_compare::Band::Ku) {
            return 51; // 0.5 倍 < 0.618 ⇒ 全枯
        }
        // 反向：三态必须互不相同（否则"三态"是假的）
        let a = l5_compare::compare(&[0.7; 4], &base)[0];
        let b = l5_compare::compare(&[1.4; 4], &base)[0];
        let c = l5_compare::compare(&[0.35; 4], &base)[0];
        if a == b || b == c || a == c {
            return 51; // 两态重合 ⇒ 判据空转
        }
    }

    // ——— 52/53：动作账链（空账自洽 → 追加后链锚改变且仍自洽 → **篡改必拒**）———
    {
        let mut l = l7::ledger::ActionLedger::new();
        if l.len() != 0 || l.head() != l7::ledger::GENESIS || !l.verify() {
            return 52;
        }
        l.append(7, l7::grade::Grade::T0Read, "suggested", "n1");
        l.append(8, l7::grade::Grade::T2Confirm, "confirmed", "n2");
        if l.len() != 2 || l.head() == l7::ledger::GENESIS || !l.verify() {
            return 52;
        }
        // 反向断言：篡改一条 ⇒ verify 必须为 false
        let keep = l.links[0].hash;
        l.links[0].hash = keep ^ 1;
        if l.verify() {
            return 53; // 篡改后仍通过 ⇒ 校验空转
        }
        l.links[0].hash = keep;
        if !l.verify() {
            return 53; // 复原后必须再次自洽
        }
    }

    // ——— 54/55：沙漏瓶颈（每 tick 至多 1 粒；放行值须在 0-1）———
    {
        let mut hg = hourglass::BubbleHourglass::with_caps(2, 2, 2, 1);
        hg.push(0.1);
        hg.push(0.2);
        hg.push(0.3); // 上锥容量 2 ⇒ 第三粒必然被丢弃
        let out = hg.tick(None);
        if out.len() > 1 {
            return 54; // 契约：每 tick 至多放行 1 粒
        }
        for v in out.iter() {
            if !(0.0..=1.0).contains(v) {
                return 55; // 放行的种子必须在 0-1（上游已 clamp01）
            }
        }
    }

    0
}

// ================== ⑦ 段（2.3b 片4）：痕迹/基因库/场域/世界/四元组/语境/证据 ==================
//
// 覆盖片4 的 10 个模块。**每条都打在"契约"上**（不是"能跑就算过"），并配**反向断言**防"算子空转"。
#[allow(clippy::too_many_lines)]
fn self_check_shard4() -> u8 {
    use meta_kernel_core_nostd::{gene_library, habit, l1_field_parse, l5_evidence, trace};

    // ① `trace::fingerprint_of`：空⇒0（明确契约）；同输入⇒同输出；**n 或 flow 变⇒必须不同**
    let s4 = [0.2f32, 0.4, 0.6, 0.8];
    let s8 = [0.2f32, 0.4, 0.6, 0.8, 0.2, 0.4, 0.6, 0.8];
    if trace::fingerprint_of(&[], 0.5) != 0 {
        return 41; // 空输入必须为 0（不是"随机的哈希"）
    }
    if trace::fingerprint_of(&s4, 0.5) != trace::fingerprint_of(&s4, 0.5) {
        return 41; // 必须确定性
    }
    if trace::fingerprint_of(&s4, 0.5) == trace::fingerprint_of(&s8, 0.5) {
        return 41; // 长度不同却同指纹 ⇒ 指纹丢了信息（低位含 n）
    }
    if trace::fingerprint_of(&s4, 0.0) == trace::fingerprint_of(&s4, 1.0) {
        return 41; // 能量流不同却同指纹 ⇒ 高位没起作用
    }

    // ② `l1_field_parse::normalize`：**半饱和点契约**（x=k ⇒ 0.5）＋ 负值归零 ＋ 单调
    if l1_field_parse::normalize(0.0, l1_field_parse::K_TEXT_CHARS) != 0.0 {
        return 42;
    }
    if l1_field_parse::normalize(l1_field_parse::K_TEXT_CHARS, l1_field_parse::K_TEXT_CHARS) != 0.5 {
        return 42; // 文档写明"达到 k 时取 0.5"——这是可判据的契约
    }
    if l1_field_parse::normalize(-5.0, l1_field_parse::K_TEXT_CHARS) != 0.0 {
        return 42; // 负值必须归零（不是负数）
    }
    if !(l1_field_parse::normalize(100.0, 2000.0) < l1_field_parse::normalize(500.0, 2000.0)) {
        return 42; // 必须单调递增
    }

    // ③ `habit::habit_strength`：count=0 ⇒ 0（无次数即无强度）；值域 [0,1]；随次数单调不减
    if habit::habit_strength(0, 1.0) != 0.0 {
        return 43;
    }
    let h1 = habit::habit_strength(1, 1.0);
    let h10 = habit::habit_strength(10, 1.0);
    let h100 = habit::habit_strength(100, 1.0);
    if !(0.0..=1.0).contains(&h1) || !(0.0..=1.0).contains(&h100) {
        return 43; // 值域
    }
    if !(h1 < h10 && h10 < h100) {
        return 43; // 必须单调递增（不是常数）
    }

    // ④ `gene_library::fnv1a64`：确定 ＋ **不同输入必须不同**（防"退化哈希"）
    let k1 = gene_library::fnv1a64(0, b"meta-kernel");
    let k2 = gene_library::fnv1a64(0, b"meta-kernel");
    let k3 = gene_library::fnv1a64(0, b"meta-kernal");
    let k4 = gene_library::fnv1a64(1, b"meta-kernel");
    if k1 != k2 {
        return 44; // 确定性
    }
    if k1 == k3 || k1 == k4 {
        return 44; // 一字节之差 / 种子之差都必须改变结果
    }

    // ⑤ `l5_evidence::adjust`：**中性点 m = 0.5 ⇒ 修正量必须恰为 0**（R7 教训）
    let g = l5_evidence::Gains::default();
    let neutral = l5_evidence::Evidence { world_match: Some(0.5), prediction_error: None };
    let adj_n = l5_evidence::adjust(0.7, &neutral, &g);
    if adj_n.world_adjust != 0.0 {
        return 45; // `2m−1` 在 m=0.5 处必须精确为 0
    }
    if adj_n.world_match != Some(0.5) {
        return 45;
    }
    // 反向：两端必须**异号且非零**（否则说明修正量恒为 0、断言在空转）
    let up = l5_evidence::adjust(0.7, &l5_evidence::Evidence { world_match: Some(1.0), prediction_error: None }, &g);
    let dn = l5_evidence::adjust(0.7, &l5_evidence::Evidence { world_match: Some(0.0), prediction_error: None }, &g);
    if !(up.world_adjust > 0.0 && dn.world_adjust < 0.0) {
        return 45;
    }
    // 最终置信度必须仍在 [0,1]
    if !(0.0..=1.0).contains(&adj_n.final_confidence) || !(0.0..=1.0).contains(&up.final_confidence) {
        return 45;
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
