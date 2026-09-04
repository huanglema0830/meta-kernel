//! Phase 1 验收测试：**10000 次迭代不溢出**（白皮书闭环压力）。
//!
//! 场景：0 锚点真空启动 → 镜像池注入第一扰动 → 气泡沙漏承载种子流
//! （含突发成对的破坏性干涉）→ 输出活动回喂镜像池 → 三引擎采样，
//! 断言全程无 NaN/无穷且始终落在 [0,1]。
//!
//! 真空重启（回显耗尽→内部第一扰动）单独特性在 `mirror` 单测中覆盖；
//! 此处验证的是**闭环稳定不溢出**。

use meta_kernel_core::{
    expo::ExpoEngine, fib::FibEngine, hourglass::BubbleHourglass, linear::LinearEngine, math::is_valid, mirror::MirrorPool, STRESS_ITERATIONS,
};

/// 确定性伪随机源（乘加线性同余，零依赖）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as u32 as f32) / (u32::MAX as f32)
    }
}

#[test]
fn closed_loop_10000_iterations_never_overflow() {
    let mut hg = BubbleHourglass::new();
    let mut pool = MirrorPool::new();
    let mut lin = LinearEngine::new();
    let mut fib = FibEngine::new();
    let mut exp = ExpoEngine::new(0.25, 1.0);
    let mut rng = Lcg(0xC0FFEE);

    let mut linear_samples = 0u32;
    let mut fib_ignited = false;
    let mut saw_life = false;

    for i in 0..STRESS_ITERATIONS {
        // 1) 偶发突发：同一 tick 连推 3 粒 → 瓶颈成对破坏性干涉
        if i % 277 == 0 {
            hg.push(1.0);
            hg.push(0.9);
            hg.push(0.8);
        }

        // 2) 外部脉冲 / 随机扰动 / 静默
        let external = if i % 97 == 0 {
            Some(1.0)
        } else if rng.next() < 0.02 {
            Some(rng.next())
        } else {
            None
        };

        // 3) 镜像池供给（无外部种子时视为停滞），优先用回显喂沙漏
        let from_pool = pool.tick(external);
        let feed = from_pool.or(external);

        // 4) 气泡沙漏承载种子流
        let outs = hg.tick(feed);
        for o in &outs {
            assert!(is_valid(*o), "hourglass out invalid at iter {i}: {o}");
            pool.observe(*o);
            saw_life |= *o > 0.0;
        }

        // 5) 三引擎采样（每 10 tick 一次）
        if i % 10 == 0 {
            let activity = outs.first().copied().unwrap_or(0.0);
            let l = lin.step(activity);
            let f = fib.step(activity);
            let x = exp.step(activity);
            assert!(is_valid(l), "linear invalid at iter {i}: {l}");
            assert!(is_valid(f), "fib invalid at iter {i}: {f}");
            assert!(is_valid(x), "expo invalid at iter {i}: {x}");
            if activity > 0.0 {
                linear_samples += 1;
            }
            if fib.latest() > 0.0 {
                fib_ignited = true;
            }
        }
    }

    // 系统确实"活过"：有回显、有破坏性干涉、有正活动、斐波那契被点燃
    assert!(pool.reflections > 0, "mirror pool never reflected");
    assert!(hg.interference_events > 0, "bursts never triggered interference");
    assert!(hg.emitted > 0, "hourglass never emitted");
    assert!(saw_life, "no positive activity in 10k iterations");
    assert!(fib_ignited, "fib engine never ignited");
    assert!(linear_samples > 0, "linear engine never sampled positive");
}

#[test]
fn each_engine_alone_10000_steps_stays_bounded() {
    // 确定性输入流：伪随机 + 周期脉冲 + 末尾 1000 步长静默（回 0 锚点）
    let mut rng = Lcg(0xDEADBEEF);
    let mut lin = LinearEngine::new();
    let mut fib = FibEngine::new();
    let mut exp = ExpoEngine::new(2.0, 1.0); // 高 λ，考验自抑制

    for i in 0..STRESS_ITERATIONS {
        let input = if i % 131 == 0 {
            1.0
        } else if i > 9000 {
            0.0
        } else {
            rng.next()
        };

        assert!(is_valid(lin.step(input)), "linear iter {i}");
        assert!(is_valid(fib.step(input)), "fib iter {i}");
        assert!(is_valid(exp.step(input)), "expo iter {i}");
    }

    // 长静默后：线性回锚点（+0.01 起步），指数真空保持 0
    assert_eq!(lin.step(0.0), 0.01);
    assert_eq!(exp.step(0.0), 0.0);
}
