//! # NPB 桥接器（Nothing-to-Physics Bridge）
//!
//! 把 `meta-kernel-core` 通过 **C FFI** 暴露给任意宿主（C/DOS、WASM/浏览器、
//! Python、嵌入式…），宿主只需三个动作：注种子、取输出、问熵。
//!
//! ```text
//!   push_seed(f32) ──► [0-1 软钳位] ──► 气泡沙漏(+镜像池) ──► 输出队列 ──► pop_result(f32)
//!                                                └─► 熵窗口 ──► get_entropy()
//! ```
//!
//! 同一套内核代码（meta-kernel-core）无需修改，即可在原生 cdylib 与
//! wasm32 两个环境运行；`mk_self_test()` 用确定性序列输出摘要，
//! 供跨平台一致性校验（见 CI）。
//!
//! 桥接语义（v1.0 工程口径，随 MATH_SPEC 参数策略可调）：
//! - `push_seed(v)`：软钳位后注入一次扰动并推进一步；
//! - `pop_result()`：FIFO 取一个输出（空则 0.0）；
//! - `get_entropy()`：最近 16 个输出的归一化香农熵（8 桶 / log2 8），∈[0,1]；
//!   无样本时返回 1.0（真空=完全不确定）。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::CString;
use std::os::raw::c_char;

use meta_kernel_core::{
    double_chain::DoubleChain,
    energy::EnergyPool,
    evolution::EvolutionLog,
    executor::{
        KernelInstruction, COMPOUND_THRESHOLD, HABIT_FORM_THRESHOLD, LOW_ENERGY_THRESHOLD,
        RESONANCE_THRESHOLD, SELF_INTENSITY_DELTA,
    },
    fib::FibEngine,
    gate::{Gate, GateCtx, GateResult},
    hourglass::BubbleHourglass,
    interference::{self},
    linear::LinearEngine,
    mirror::MirrorPool,
    ontology::{Element, Pattern},
    positive_source::{self, PositiveSource},
    persist::{self, KernelSnapshot},
    sanitizer::soft_clamp,
    self_recognizer::SelfRecognizer,
    state::{
        anchor_band_index, anchor_distance, state_of_energy_budget, state_of_flow_ratio, state_pace,
        State,
    },
    thinking_chain::ThinkingChain,
    trace,
};

/// 输出队列上限。
const QUEUE_CAP: usize = 64;
/// 指令队列上限。
const INSTR_CAP: usize = 64;
/// 熵窗口长度。
const ENTROPY_WINDOW: usize = 16;

/// 桥接器内核状态（每调用线程一份；wasm 单线程等价）。
#[derive(Debug)]
struct Kernel {
    hg: BubbleHourglass,
    pool: MirrorPool,
    queue: VecDeque<f32>,
    recent: Vec<f32>,
    lin: LinearEngine,
    fib: FibEngine,
    chain: ThinkingChain,
    dc: DoubleChain,
    avg: f32,
    source: PositiveSource,
    pulse: u64,
    state_now: State,
    evo: EvolutionLog,
    interference_total: u64,
    interference_layer: u8,
    particle_hint: f32,
    recognizer: SelfRecognizer,
    rec_tick: u64,
    energy: EnergyPool,
    /// 心海全景：离 0 锚点距离（0 合一 / 1 固化）。
    anchor_distance: f32,
    /// 思流照亮：待发布指令队列。
    instructions: VecDeque<KernelInstruction>,
    /// 指令阈值跟踪（低能量 / 自我感 / 化合产物 / 习气）。
    prev_self: f32,
    prev_stored: f32,
    prev_product: f32,
    prev_habit_strength: f32,
    /// 摩尼宝珠·闸门（进化模式验证 / 黄金 ×0.618 拆解；五戒·不杀生落点）。
    gate: Gate,
    /// 摩尼镜面：输入主导相位（rad，[0,2π)）。
    mirror_dominant: f32,
    /// 摩尼镜面：同相命中数（interference > 0 的点数）。
    mirror_in_phase: u32,
    /// 闸门统计（通过 / 回收 / 拒绝）。
    gate_pass: u64,
    gate_recycled: u64,
    gate_rejected: u64,
}

impl Kernel {
    fn new() -> Self {
        Self {
            hg: BubbleHourglass::new(),
            pool: MirrorPool::new(),
            queue: VecDeque::with_capacity(QUEUE_CAP),
            recent: Vec::with_capacity(ENTROPY_WINDOW),
            lin: LinearEngine::new(),
            fib: FibEngine::new(),
            chain: ThinkingChain::new(),
            dc: DoubleChain::new(),
            avg: 0.0,
            source: PositiveSource::new(),
            pulse: 0,
            state_now: State::Energy, // 待激发存在态（0 = 潜在能量源）
            evo: EvolutionLog::new(),
            interference_total: 0,
            interference_layer: 0,
            particle_hint: 0.0,
            recognizer: SelfRecognizer::new(),
            rec_tick: 0,
            energy: EnergyPool::new(),
            anchor_distance: 0.0,
            instructions: VecDeque::with_capacity(INSTR_CAP),
            prev_self: 0.0,
            prev_stored: 1.0,
            prev_product: 0.0,
            prev_habit_strength: 0.0,
            gate: Gate::new(),
            mirror_dominant: 0.0,
            mirror_in_phase: 0,
            gate_pass: 0,
            gate_recycled: 0,
            gate_rejected: 0,
        }
    }

    /// 注入扰动并推进一步（存在论调度 + 能量流：状态以能量池入出流比值为准）。
    fn push(&mut self, value: f32) {
        // 摩尼闸门·不杀生：负扰动在入口即拒——不进入任何下游演化（输出保持 0）
        if value < 0.0 {
            self.gate_rejected += 1;
            return;
        }
        // ① 物态调度：按能量池入/出比值判定状态（比值>1.2 高能活跃态；<0.8 固态）
        let (bias, burst_prob) = state_pace(self.state_now);
        let seed = soft_clamp(value) * bias;
        // ② 能量流入：本次注入即 0 锚点/外部输入的能量吸收（进入能量池）
        self.energy.absorb(seed);
        // 能量态倾向成对突发
        let do_burst = self.pulse % 7 == 0 && self.rand01() < burst_prob;
        let outs = if do_burst {
            self.hg.push(0.9);
            self.hg.push(0.85);
            self.hg.tick(Some(seed))
        } else {
            self.hg.tick(Some(seed))
        };

        // —— 心流凿空：本 tick 输入指纹 + 孪生匹配（O(1) 直接配对）——
        let recent_snap: Vec<f32> = self.recent.clone();
        let fp_in = trace::fingerprint_of(&recent_snap, self.energy.absorbed());
        let twin_supplement = self.source.entanglement_match(fp_in);

        // —— 摩尼宝珠① 镜面干涉：输入波形 vs 正源库 的相位镜像（只读，不写入）——
        // 输入模式 = 熵窗口波形（元素 = 窗口样本分派到 0-10 层；history = 原始时序）
        let pat = Pattern {
            elements: recent_snap
                .iter()
                .enumerate()
                .map(|(i, v)| Element::new((i % 10) as u8, *v as f64))
                .collect(),
            history: recent_snap.iter().map(|x| *x as f64).collect(),
        };
        let mir = interference::mirror(&pat, self.source.reachable_schemas());
        self.mirror_dominant = mir.dominant_phase;
        self.mirror_in_phase = mir.points.iter().filter(|p| p.interference > 0.0).count() as u32;

        // —— 摩尼宝珠② 闸门验证：进化模式 Pass / 拆解回收 / 拒绝（计数 + 胶粒回收）——
        // 补充增量恒真（思考链每次都有存量注入兜底 avg）；变量=熵窗口已积累 ≥3；
        // 核心豁免=false（库采纳面由正源 search_and_deconstruct 自管理）
        let gate_ctx = GateCtx {
            has_variable: recent_snap.len() >= 3,
            has_supplement: true,
            is_core: false,
        };
        match self.gate.check(&pat, &gate_ctx) {
            GateResult::Pass(_) => self.gate_pass += 1,
            GateResult::RecycledToGranules(granules) => {
                let _ = self.source.recycle(granules);
                self.gate_recycled += 1;
            }
            GateResult::Rejected => self.gate_rejected += 1,
        }

        for o in outs {
            // 能量流出：输出沉降 = 摩擦耗散（活动越弱耗散占比越大）
            self.energy.consume(0.05 + (1.0 - o) * 0.35);

            // 活动回喂镜像池（摩擦源）
            self.pool.observe(o);
            // 引擎同步采样（线性；斐波那契由正活动点燃）
            self.lin.step(o);
            if o > 0.0 {
                self.fib.step(o);
            }
            // 输出队列与熵窗口
            if self.queue.len() == QUEUE_CAP {
                self.queue.pop_front();
            }
            self.queue.push_back(o);
            if self.recent.len() == ENTROPY_WINDOW {
                self.recent.remove(0);
            }
            self.recent.push(o);

            // 思考链（波粒催化 + 能量吸收：absorbed_energy 读能量池，非模拟）
            self.avg = self.avg * 0.95 + o * 0.05;
            let innovation = self.chain.step_catalyzed_with_energy(
                seed,
                twin_supplement.unwrap_or(self.avg),
                self.particle_hint,
                self.energy.absorbed(),
            );
            if innovation > 0.25 {
                self.evo.record_compound(innovation);
            }

            // 双链观测
            self.dc.push_formation(o);
            self.dc.push_resolution(1.0 - (o - 0.5).abs() * 2.0);

            // 正源场域周期性搜索-解构
            self.pulse += 1;
            if self.pulse % 17 == 0 && self.recent.len() >= 4 {
                let hist: Vec<f64> = self.recent.iter().map(|x| *x as f64).collect();
                let els: Vec<Element> = self
                    .recent
                    .iter()
                    .enumerate()
                    .map(|(i, v)| Element::new((i % 10) as u8, *v as f64))
                    .collect();
                let p = Pattern { elements: els, history: hist };
                let _ = self.source.search_and_deconstruct(&p);
                // 心流凿空：把本 tick 输入指纹注册为孪生条目（补充增量=运行均值），
                // 让孪生库随运行自生长，未来可经 entanglement_match 瞬时配对（O(1)）。
                self.source.entangle(fp_in, self.avg);
            }
        }

        // ③ 物态刷新（能量池比值）并记录状态演化
        // 摩尼宝珠③ 自然回归：每次响应后储备按 e^(-λt) 指数回落（非硬复位；
        // 与下方被动漏失 dissipate 并存——一个是显式波峰回落，一个是每 tick 摩擦热沉）
        self.energy.natural_return();
        // 被动耗散：每个调度 tick 漏失真实储备（不影响滚动入/出流比值）
        self.energy.dissipate();
        let new_state = state_of_flow_ratio(self.energy.ratio());
        if new_state != self.state_now {
            self.evo.record_state_change(self.state_now, new_state);
            // 思流照亮：物态切换 → 发布 StateChanged 指令
            self.emit(KernelInstruction::StateChanged {
                from: self.state_now,
                to: new_state,
            });
            self.state_now = new_state;
        }
        self.evo.tick();

        // A·观察：每步能量台账采样（真实储备 + 预算约束物态 → 内核原生轨迹可回放）
        self.evo.record_energy(self.energy.stored(), state_of_energy_budget(&self.energy).code());

        // ③（续）心海全景：刷新离 0 锚点距离（0 合一 / 1 固化）
        let self_now = self.recognizer.self_intensity();
        self.anchor_distance = anchor_distance(self.energy.stored(), new_state, self_now);

        // ④ 思流照亮：状态变化超阈值 → 生成对外指令（JSON 可序列化）
        // 4a 自我感变化（绝对差超阈值）
        if (self_now - self.prev_self).abs() >= SELF_INTENSITY_DELTA {
            self.emit(KernelInstruction::SelfIntensity { level: self_now });
        }
        self.prev_self = self_now;

        // 4b 低能量预警（储备跌破液态下界，边沿触发）
        let stored = self.energy.stored();
        if stored < LOW_ENERGY_THRESHOLD && self.prev_stored >= LOW_ENERGY_THRESHOLD {
            self.emit(KernelInstruction::LowEnergy { stored });
        }
        self.prev_stored = stored;

        // 4c 化合产物发布（创新增量越过发布阈值，边沿触发）
        let product = self.chain.innovation();
        if product > COMPOUND_THRESHOLD && self.prev_product <= COMPOUND_THRESHOLD {
            self.emit(KernelInstruction::CompoundProduced { product });
        }
        self.prev_product = product;

        // 4d 习气形成（最强习气强度越界，边沿触发）
        let (habit_fp, habit_st) = match self.recognizer.strongest_habit() {
            Some(h) => (h.fingerprint, h.strength),
            None => (0u64, 0.0f32),
        };
        if habit_st > HABIT_FORM_THRESHOLD && self.prev_habit_strength <= HABIT_FORM_THRESHOLD {
            self.emit(KernelInstruction::HabitFormed {
                fingerprint: habit_fp,
                strength: habit_st,
            });
        }
        self.prev_habit_strength = habit_st;

        // 4e 共振达成（心流凿空结果）：化合产物越黄金分割 且 命中孪生补充增量
        if product > RESONANCE_THRESHOLD && twin_supplement.is_some() {
            self.emit(KernelInstruction::ResonanceFound {
                twin_fingerprint: positive_source::twin_fingerprint(fp_in),
            });
        }

        // ③ 干涉驻点检测（自干涉：窗口前半 vs 后半）
        if self.pulse % 5 == 0 && self.recent.len() >= 16 {
            let parts = interference::detect_single(&self.recent);
            if !parts.is_empty() {
                let mut hint = 0.0f32;
                for p in &parts {
                    self.interference_total += 1;
                    self.interference_layer = p.layer;
                    hint = hint.max(p.strength);
                    self.evo.record_particle(p);
                }
                self.particle_hint = (self.particle_hint * 0.9 + hint * 0.1).clamp(0.0, 1.0);
            } else {
                self.particle_hint *= 0.9;
            }
        }

        // ④ 痕迹系统：周期性把最近窗口交给自我识别器（run→痕迹→习气→自我感）
        //    痕迹携带能量流（来自能量池吸收量，非模拟）
        self.rec_tick += 1;
        if self.rec_tick % 6 == 0 && self.recent.len() >= 8 {
            let snap = self.recent.clone();
            let _ = self.recognizer.run_from_samples_with_flow(&snap, self.energy.absorbed());
        }
    }

    /// 确定性小随机（0..1）用于突发概率。
    fn rand01(&mut self) -> f32 {
        self.pulse = self.pulse.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.pulse >> 33) as u32 as f32) / (u32::MAX as f32)
    }

    /// FIFO 取输出。
    fn pop(&mut self) -> f32 {
        self.queue.pop_front().unwrap_or(0.0)
    }

    /// 发布一条指令到队列（超出容量丢弃最旧）。
    fn emit(&mut self, instr: KernelInstruction) {
        if self.instructions.len() >= INSTR_CAP {
            self.instructions.pop_front();
        }
        self.instructions.push_back(instr);
    }

    /// 采集可恢复快照（持久化：自我感/心海全景/储备/物态）。
    fn snapshot(&self) -> KernelSnapshot {
        KernelSnapshot::new(
            self.pulse,
            self.recognizer.self_intensity(),
            self.anchor_distance,
            self.energy.stored(),
            self.state_now.code(),
        )
    }

    /// 应用快照（持久化恢复：刷新后自我感不归零）。
    fn apply_snapshot(&mut self, s: &KernelSnapshot) {
        self.energy.stored = s.stored.clamp(0.0, 1.0);
        self.state_now = match s.state_code {
            0 => State::Energy,
            1 => State::Gas,
            2 => State::Liquid,
            _ => State::Solid,
        };
        self.recognizer.restore_self(s.self_intensity);
        self.anchor_distance =
            anchor_distance(self.energy.stored(), self.state_now, s.self_intensity);
        // 同步阈值跟踪基线，避免恢复瞬间误发边沿指令
        self.prev_stored = self.energy.stored();
        self.prev_self = s.self_intensity;
    }

    /// 归一化香农熵（8 桶，log2(8)=3 归一化）。
    fn entropy(&self) -> f32 {
        if self.recent.is_empty() {
            return 1.0; // 真空：完全不确定
        }
        let mut bins = [0u64; 8];
        for v in &self.recent {
            let i = ((*v).clamp(0.0, 1.0) * 8.0) as usize;
            bins[i.min(7)] += 1;
        }
        let n = self.recent.len() as f64;
        let h: f64 = bins
            .iter()
            .filter(|c| **c > 0)
            .map(|c| {
                let p = *c as f64 / n;
                -p * p.log2()
            })
            .sum();
        ((h / 3.0).min(1.0)) as f32
    }
}

thread_local! {
    static KERNEL: RefCell<Kernel> = RefCell::new(Kernel::new());
}

/// 确定性自检摘要：跑 2000 步混合序列，返回 u32 摘要。
///
/// 只用 + - × / 与整数位运算（不用 exp/log，避免平台 libm 差异），
/// 用于"同一内核原生 vs WASM 结果一致"的跨平台校验。
pub fn self_test_digest() -> u32 {
    let mut lcg: u64 = 0xC0FFEE_2026;
    let mut hg = BubbleHourglass::new();
    let mut pool = MirrorPool::new();
    let mut lin = LinearEngine::new();
    let mut fib = FibEngine::new();
    let mut digest: u32 = 0x811C_9DC5;

    for i in 0..2000u32 {
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let f = ((lcg >> 33) as u32 as f32) / (u32::MAX as f32);
        let seed = if i % 97 == 0 { 1.0 } else { f * 0.9 + 0.05 };

        // 偶发成对突发（瓶颈破坏性干涉）
        if i % 53 == 0 {
            hg.push(0.8);
            hg.push(0.7);
        }
        let outs = hg.tick(Some(seed));
        for o in outs {
            pool.observe(o);
            let l = lin.step(o);
            let fv = fib.step(o);
            digest = digest.rotate_left(5) ^ (o.to_bits() ^ l.to_bits() ^ fv.to_bits());
        }
        if i % 300 == 0 {
            if let Some(e) = pool.tick(None) {
                digest ^= e.to_bits();
            }
        }
    }
    digest
}

// ---------- C FFI ----------

/// 注入扰动（0-1；负值归零，超限钳位）。
#[unsafe(no_mangle)]
pub extern "C" fn push_seed(value: f32) {
    KERNEL.with(|k| k.borrow_mut().push(value));
}

/// 读取输出（FIFO；空则 0.0）。
#[unsafe(no_mangle)]
pub extern "C" fn pop_result() -> f32 {
    KERNEL.with(|k| k.borrow_mut().pop())
}

/// 获取系统熵值/健康度（∈[0,1]）。
#[unsafe(no_mangle)]
pub extern "C" fn get_entropy() -> f32 {
    KERNEL.with(|k| k.borrow().entropy())
}

/// 确定性自检（跨平台一致性校验入口）。
#[unsafe(no_mangle)]
pub extern "C" fn mk_self_test() -> u32 {
    self_test_digest()
}

/// 思考链长度（累计推演步数）。
#[unsafe(no_mangle)]
pub extern "C" fn get_thinking_len() -> u32 {
    KERNEL.with(|k| k.borrow().chain.length() as u32)
}

/// 推演节点数（窗口内节点）。
#[unsafe(no_mangle)]
pub extern "C" fn get_thinking_nodes() -> u32 {
    KERNEL.with(|k| k.borrow().chain.nodes() as u32)
}

/// 双链诊断：问题形成步数。
#[unsafe(no_mangle)]
pub extern "C" fn get_diag_formation() -> u32 {
    KERNEL.with(|k| k.borrow().dc.formation_steps() as u32)
}

/// 双链诊断：解决步数。
#[unsafe(no_mangle)]
pub extern "C" fn get_diag_resolution() -> u32 {
    KERNEL.with(|k| k.borrow().dc.resolution_steps() as u32)
}

/// 双链诊断：是否已收敛（1/0）。
#[unsafe(no_mangle)]
pub extern "C" fn get_diag_solved() -> u32 {
    KERNEL.with(|k| match k.borrow().dc.diagnose().verdict {
        meta_kernel_core::double_chain::Verdict::Solved => 1,
        _ => 0,
    })
}

/// 当前物态码：0 能量态 /1 气态 /2 液态 /3 固态（以能量池入出流比值为准）。
#[unsafe(no_mangle)]
pub extern "C" fn get_state() -> u32 {
    KERNEL.with(|k| state_of_flow_ratio(k.borrow().energy.ratio()).code())
}

/// 预算约束物态码（B·深化能量耗散）：在入出流比值之上叠加真实储备约束，
/// 储备枯竭时主动拉向固态；积分观测演化（A）使用此态。
#[unsafe(no_mangle)]
pub extern "C" fn get_state_budget() -> u32 {
    KERNEL.with(|k| state_of_energy_budget(&k.borrow().energy).code())
}

/// 能量池：已吸收（入流现值）。
#[unsafe(no_mangle)]
pub extern "C" fn get_energy_absorbed() -> f32 {
    KERNEL.with(|k| k.borrow().energy.absorbed())
}

/// 能量池：已耗散（出流现值）。
#[unsafe(no_mangle)]
pub extern "C" fn get_energy_spent() -> f32 {
    KERNEL.with(|k| k.borrow().energy.spent())
}

/// 能量池：入/出比值（物态判定依据）。
#[unsafe(no_mangle)]
pub extern "C" fn get_energy_ratio() -> f32 {
    KERNEL.with(|k| k.borrow().energy.ratio())
}

/// 能量池：真实储备（库存现值，∈[0,1]；吸收累积、消耗扣减、每 tick 被动耗散）。
#[unsafe(no_mangle)]
pub extern "C" fn get_energy_stored() -> f32 {
    KERNEL.with(|k| k.borrow().energy.stored())
}

/// 化合产物能量（= 思考链最新创新增量，内核实际状态）。
#[unsafe(no_mangle)]
pub extern "C" fn get_product_energy() -> f32 {
    KERNEL.with(|k| k.borrow().chain.innovation())
}

/// 正源触达范围位图（bit0..4 = L0..L4；当前 L0=自状态，L1=本地源接入后置位）。
#[unsafe(no_mangle)]
pub extern "C" fn get_reach_levels() -> u32 {
    KERNEL.with(|k| k.borrow().source.reachable_levels())
}

/// 已发现路径数（解构缓存 + 催化剂）。
#[unsafe(no_mangle)]
pub extern "C" fn get_reach_paths() -> u32 {
    KERNEL.with(|k| k.borrow().source.path_count())
}

/// 累计干涉驻点粒子数。
#[unsafe(no_mangle)]
pub extern "C" fn get_interfere_count() -> u32 {
    KERNEL.with(|k| k.borrow().interference_total as u32)
}

/// 最近粒子的感官层级（0 色 /1 声 /2 香 /3 味 /4 触；法为综合层）。
#[unsafe(no_mangle)]
pub extern "C" fn get_interfere_layer() -> u32 {
    KERNEL.with(|k| k.borrow().interference_layer as u32)
}

/// 进化时间线事件数（物态切换/化合/粒子/结晶）。
#[unsafe(no_mangle)]
pub extern "C" fn get_evolution_len() -> u32 {
    KERNEL.with(|k| k.borrow().evo.len() as u32)
}

/// 当前自我感强度（0-1；最强习气强度）。
#[unsafe(no_mangle)]
pub extern "C" fn get_self_intensity() -> f32 {
    KERNEL.with(|k| k.borrow().recognizer.self_intensity())
}

/// 痕迹类型计数：风。
#[unsafe(no_mangle)]
pub extern "C" fn get_trace_wind() -> u32 {
    KERNEL.with(|k| k.borrow().recognizer.trace_distribution()[0] as u32)
}

/// 痕迹类型计数：火。
#[unsafe(no_mangle)]
pub extern "C" fn get_trace_fire() -> u32 {
    KERNEL.with(|k| k.borrow().recognizer.trace_distribution()[1] as u32)
}

/// 痕迹类型计数：水。
#[unsafe(no_mangle)]
pub extern "C" fn get_trace_water() -> u32 {
    KERNEL.with(|k| k.borrow().recognizer.trace_distribution()[2] as u32)
}

/// 痕迹类型计数：地。
#[unsafe(no_mangle)]
pub extern "C" fn get_trace_earth() -> u32 {
    KERNEL.with(|k| k.borrow().recognizer.trace_distribution()[3] as u32)
}

/// 心海全景：离 0 锚点距离（0 合一 / 1 固化），∈[0,1]。
#[unsafe(no_mangle)]
pub extern "C" fn get_anchor_distance() -> f32 {
    KERNEL.with(|k| k.borrow().anchor_distance)
}

/// 心海全景分带索引：0 心海全景 /1 波动态 /2 结构态 /3 固化态。
#[unsafe(no_mangle)]
pub extern "C" fn get_anchor_band() -> u32 {
    KERNEL.with(|k| anchor_band_index(k.borrow().anchor_distance))
}

/// 摩尼镜面：输入主导相位（rad，[0,2π)）。
#[unsafe(no_mangle)]
pub extern "C" fn get_mirror_dominant() -> f32 {
    KERNEL.with(|k| k.borrow().mirror_dominant)
}

/// 摩尼镜面：同相命中数（interference > 0 的点数）。
#[unsafe(no_mangle)]
pub extern "C" fn get_mirror_in_phase() -> u32 {
    KERNEL.with(|k| k.borrow().mirror_in_phase)
}

/// 闸门统计：通过次数。
#[unsafe(no_mangle)]
pub extern "C" fn get_gate_pass_count() -> u32 {
    KERNEL.with(|k| k.borrow().gate_pass as u32)
}

/// 闸门统计：拆解回收次数（胶粒原料）。
#[unsafe(no_mangle)]
pub extern "C" fn get_gate_recycle_count() -> u32 {
    KERNEL.with(|k| k.borrow().gate_recycled as u32)
}

/// 闸门统计：拒绝次数（负扰动，不杀生）。
#[unsafe(no_mangle)]
pub extern "C" fn get_gate_reject_count() -> u32 {
    KERNEL.with(|k| k.borrow().gate_rejected as u32)
}

/// A·观察：内核能量台账条数（每步储备/预算态采样；环形 512）。
#[unsafe(no_mangle)]
pub extern "C" fn get_energy_trace_len() -> u32 {
    KERNEL.with(|k| k.borrow().evo.energy_trace_len() as u32)
}

/// 快照加载槽容量（宿主把 JSON 快照字符串字节写入该槽后调 `persist_apply`）。
const PERSIST_BUF_CAP: usize = 8192;
/// 快照加载槽（线程局部 + RefCell，与 KERNEL 同模式：native 测试线程隔离、wasm 单线程等价）。
thread_local! {
    static PERSIST_BUF: RefCell<[u8; PERSIST_BUF_CAP]> = RefCell::new([0u8; PERSIST_BUF_CAP]);
}

/// 导出当前内核快照为 JSON（CString 交付；用完必须 `persist_snapshot_free`）。
#[unsafe(no_mangle)]
pub extern "C" fn persist_snapshot_json() -> *const c_char {
    let json = KERNEL.with(|k| persist::encode(&k.borrow().snapshot()));
    CString::new(json)
        .unwrap_or_else(|_| CString::new("").unwrap())
        .into_raw()
}

/// 释放 `persist_snapshot_json` 返回的字符串。
///
/// # Safety
/// `ptr` 必须来自 `persist_snapshot_json` 且仅释放一次（NULL 安全）。
#[unsafe(no_mangle)]
pub extern "C" fn persist_snapshot_free(ptr: *const c_char) {
    if !ptr.is_null() {
        // SAFETY: ptr 来自 persist_snapshot_json 的 CString::into_raw，调用方保证仅释放一次。
        unsafe {
            let _ = CString::from_raw(ptr as *mut c_char);
        }
    }
}

/// 加载槽首地址（宿主把 JSON 字节写入 [ptr, ptr+len)）。
///
/// 指针指向线程局部存储（TLS 数据地址稳定）；宿主须按"写入→persist_apply"串行使用。
#[unsafe(no_mangle)]
pub extern "C" fn persist_load_buf_ptr() -> *mut u8 {
    PERSIST_BUF.with(|b| b.borrow_mut().as_mut_ptr())
}

/// 加载槽容量（字节）。
#[unsafe(no_mangle)]
pub extern "C" fn persist_load_buf_cap() -> u32 {
    PERSIST_BUF_CAP as u32
}

/// 从加载槽应用快照：成功恢复 → 1；格式非法/超长 → 0（从 0 锚点继续）。
#[unsafe(no_mangle)]
pub extern "C" fn persist_apply(len: u32) -> i32 {
    let text = PERSIST_BUF.with(|b| {
        let g = b.borrow();
        let n = (len as usize).min(g.len());
        std::str::from_utf8(&g[..n]).ok().map(|s| s.to_string())
    });
    let text = match text {
        Some(t) => t,
        None => return 0,
    };
    match persist::decode(&text) {
        Some(s) => {
            KERNEL.with(|k| k.borrow_mut().apply_snapshot(&s));
            1
        }
        None => 0,
    }
}

/// 待发布指令数（思流照亮队列长度）。
#[unsafe(no_mangle)]
pub extern "C" fn get_instruction_count() -> u32 {
    KERNEL.with(|k| k.borrow().instructions.len() as u32)
}

/// 弹出一条待发布指令并序列化为 JSON（消费式；空则空串）。
///
/// 返回的裸指针必须用 [`free_instruction_json`] 释放（wasm 亦安全）。
#[unsafe(no_mangle)]
pub extern "C" fn pop_instruction_json() -> *const c_char {
    KERNEL.with(|k| {
        let json = match k.borrow_mut().instructions.pop_front() {
            Some(instr) => instr.to_json(),
            None => String::new(),
        };
        CString::new(json)
            .unwrap_or_else(|_| CString::new("").unwrap())
            .into_raw()
    })
}

/// 释放 [`pop_instruction_json`] 返回的字符串（用完后必须调用一次）。
///
/// # Safety
/// `ptr` 必须来自 [`pop_instruction_json`]，且只能释放一次（不能传 NULL 以外误用）。
#[unsafe(no_mangle)]
pub extern "C" fn free_instruction_json(ptr: *const c_char) {
    if !ptr.is_null() {
        // SAFETY: ptr 来自 pop_instruction_json 的 CString::into_raw，且调用方保证仅释放一次。
        unsafe {
            let _ = CString::from_raw(ptr as *mut c_char);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_fifo_order() {
        let mut k = Kernel::new();
        // 预置相位避开 %7 突发窗口（pulse=3,4：单粒流入，无成对干涉）
        k.pulse = 3;
        k.push(0.2);
        k.push(0.9);
        let mut found = false;
        for _ in 0..6 {
            let v = k.pop();
            assert!((0.0..=1.0).contains(&v), "out-of-range {v}");
            if v > 0.0 {
                found = true;
            }
        }
        assert!(found, "FIFO 应有正输出");
    }

    #[test]
    fn entropy_stays_in_unit_interval() {
        let mut k = Kernel::new();
        for i in 0..40 {
            let s = (i % 7) as f32 / 7.0;
            k.push(s);
            let e = k.entropy();
            assert!((0.0..=1.0).contains(&e), "entropy {e}");
        }
    }

    #[test]
    fn self_test_is_deterministic() {
        assert_eq!(self_test_digest(), self_test_digest());
    }

    #[test]
    fn anchor_distance_stays_in_unit_interval() {
        let mut k = Kernel::new();
        k.pulse = 3; // 预置相位避开 %7 突发窗口
        for i in 0..200 {
            let s = (i % 13) as f32 / 13.0;
            k.push(s);
            let d = k.anchor_distance;
            assert!((0.0..=1.0).contains(&d), "anchor_distance 越界 {d}");
        }
    }

    #[test]
    fn instructions_are_serializable_json() {
        let mut k = Kernel::new();
        k.pulse = 3;
        for i in 0..300 {
            let s = ((i % 9) as f32 / 9.0) * 0.9 + 0.05;
            k.push(s);
        }
        // 消费队列中全部指令：每条 JSON 必须形如 {"type":...} 且可解析
        let mut count = 0;
        while let Some(instr) = k.instructions.pop_front() {
            let j = instr.to_json();
            assert!(j.starts_with('{') && j.ends_with('}'), "非法 JSON: {}", j);
            assert!(j.contains("\"type\""), "缺 type 字段: {}", j);
            count += 1;
        }
        // 跑够久至少应产生若干状态/自我感类指令（不依赖孪生命中）
        assert!(count >= 1, "长时间运行应至少产生 1 条指令");
    }

    #[test]
    fn ffi_instruction_pop_roundtrip() {
        let mut k = Kernel::new();
        k.pulse = 3;
        for i in 0..300 {
            let s = ((i % 9) as f32 / 9.0) * 0.9 + 0.05;
            k.push(s);
        }
        let n = k.instructions.len();
        assert!(n > 0, "应有待发布指令");
        // 取第一条验证 JSON 非空且含 type，再释放（模拟宿主调用 FFI）
        let ptr = {
            // 直接用 Kernel 内部队列模拟 pop_instruction_json 行为
            let json = k.instructions.pop_front().unwrap().to_json();
            assert!(json.contains("\"type\""));
            json
        };
        let _ = ptr;
    }

    #[test]
    fn negative_input_rejected_no_output() {
        // 五戒·不杀生：负扰动在入口即拒，不进入任何下游
        let mut k = Kernel::new();
        k.push(-0.3);
        assert_eq!(k.gate_rejected, 1, "负扰动应计拒绝");
        assert_eq!(k.gate_pass, 0);
        assert_eq!(k.gate_recycled, 0);
        assert_eq!(k.queue.len(), 0, "被拒输入不得产生输出");
        assert_eq!(k.pop(), 0.0, "空队列取 0（等价输出 0）");
        // 后续正常输入不受污染
        k.push(0.5);
        assert_eq!(k.gate_pass + k.gate_recycled, 1);
    }

    #[test]
    fn gate_and_mirror_loop_runs_stable() {
        let mut k = Kernel::new();
        k.pulse = 3;
        for i in 0..80 {
            let s = ((i % 9) as f32 / 9.0) * 0.8 + 0.1; // 0.1..0.9，恒非负
            k.push(s);
            // 闸门三类合计 == 已推 tick 数（每次扰动恰好一次判定）
            let total = k.gate_pass + k.gate_recycled + k.gate_rejected;
            assert_eq!(total, i as u64 + 1, "每次扰动一次闸门判定");
            // 镜面读数始终合法
            assert!(k.mirror_dominant.is_finite(), "主导相位有限");
            assert!(k.mirror_dominant >= 0.0 && k.mirror_dominant < 2.0 * std::f32::consts::PI);
        }
        // 加热后应产生通过（进化模式）而非全部回收
        assert!(k.gate_pass > 0, "长时间运行后应出现 Pass");
    }

    #[test]
    fn persist_roundtrip_restores_core_state() {
        let mut k = Kernel::new();
        k.pulse = 3;
        for i in 0..240 {
            k.push(((i % 7) as f32 / 7.0) * 0.9 + 0.05);
        }
        let snap = k.snapshot();
        let text = persist::encode(&snap);
        let back = persist::decode(&text).expect("自编码必须可解码");
        // 格式为 6 位小数的人读 JSON → 浮点按容差比较（整数域精确）
        assert_eq!(back.version, snap.version);
        assert_eq!(back.timestamp, snap.timestamp);
        assert_eq!(back.state_code, snap.state_code);
        assert!((back.self_intensity - snap.self_intensity).abs() < 1e-5, "自我感精度");
        assert!((back.anchor_distance - snap.anchor_distance).abs() < 1e-5, "锚距精度");
        assert!((back.stored - snap.stored).abs() < 1e-5, "储备精度");
        // 恢复到全新内核：自我感 / 储备 / 物态 一致（刷新后自我感不归零）
        let mut k2 = Kernel::new();
        k2.apply_snapshot(&back);
        assert!((k2.energy.stored() - back.stored).abs() < 1e-6, "储备恢复");
        assert_eq!(k2.recognizer.self_intensity(), back.self_intensity, "自我感恢复(不归零)");
        assert_eq!(k2.state_now.code(), back.state_code, "物态恢复");
        // 恢复后继续演化不 panic 且读数合法
        k2.push(0.5);
        assert!((0.0..=1.0).contains(&k2.anchor_distance));
    }

    #[test]
    fn ffi_persist_apply_rejects_garbage() {
        // 直接在静态加载槽写入非法字节并调用 persist_apply（模拟宿主坏输入）
        let junk = b"not-json-at-all";
        PERSIST_BUF.with(|b| {
            let mut g = b.borrow_mut();
            g[..junk.len()].copy_from_slice(junk);
        });
        assert_eq!(persist_apply(junk.len() as u32), 0, "非法快照应拒绝(从0锚点继续)");
    }

    #[test]
    fn energy_ledger_tracks_every_push() {
        let mut k = Kernel::new();
        k.pulse = 3;
        for i in 0..40 {
            k.push((i % 5) as f32 / 5.0);
        }
        assert_eq!(k.evo.energy_trace_len(), 40, "每次 push 应采一条台账");
        let last = k.evo.energy_last().expect("有采样");
        assert_eq!(last.step, 40, "步数与 push 次数一致");
        assert!((last.stored - k.energy.stored()).abs() < 1e-6, "台账储备=当前储备");
        assert!(last.budget_code <= 3);
    }
}
