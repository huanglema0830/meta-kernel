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

use meta_kernel_core::{
    double_chain::DoubleChain,
    energy::EnergyPool,
    evolution::EvolutionLog,
    fib::FibEngine,
    hourglass::BubbleHourglass,
    interference::{self},
    linear::LinearEngine,
    mirror::MirrorPool,
    ontology::{Element, Pattern},
    positive_source::PositiveSource,
    sanitizer::soft_clamp,
    self_recognizer::SelfRecognizer,
    state::{state_of_flow_ratio, state_pace, State},
    thinking_chain::ThinkingChain,
};

/// 输出队列上限。
const QUEUE_CAP: usize = 64;
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
        }
    }

    /// 注入扰动并推进一步（存在论调度 + 能量流：状态以能量池入出流比值为准）。
    fn push(&mut self, value: f32) {
        // ① 物态调度：按能量池入/出比值判定状态（比值>1.2 高能活跃态；<0.8 固态）
        let (bias, burst_prob) = state_pace(self.state_now);
        let mut seed = soft_clamp(value) * bias;
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
                self.avg,
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
            }
        }

        // ③ 物态刷新（能量池比值）并记录状态演化
        let new_state = state_of_flow_ratio(self.energy.ratio());
        if new_state != self.state_now {
            self.evo.record_state_change(self.state_now, new_state);
            self.state_now = new_state;
        }
        self.evo.tick();

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
}
