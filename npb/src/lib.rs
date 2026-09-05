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
    fib::FibEngine,
    hourglass::BubbleHourglass,
    linear::LinearEngine,
    mirror::MirrorPool,
    sanitizer::soft_clamp,
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
        }
    }

    /// 注入扰动并推进一步。
    fn push(&mut self, value: f32) {
        let seed = soft_clamp(value);
        let outs = self.hg.tick(Some(seed));
        for o in outs {
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

            // 思考链：每轮活动 = 一次推演（存量=上轮创新增量降维，变量=注入，补充=滑动均值）
            self.avg = self.avg * 0.95 + o * 0.05;
            self.chain.step(seed, self.avg);

            // 双链观测：活动值写入"问题形成"轨迹；对锚点(0.5)的贴近度写入"解决"轨迹
            self.dc.push_formation(o);
            self.dc.push_resolution(1.0 - (o - 0.5).abs() * 2.0);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_fifo_order() {
        let mut k = Kernel::new();
        k.push(0.2);
        k.push(0.9);
        let a = k.pop();
        let b = k.pop();
        assert!((a + b) > 0.0, "FIFO 应有输出");
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
