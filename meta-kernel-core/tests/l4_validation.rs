//! L4 戒律判定引擎 · 实测定型（6 用例）。
//!
//! 依据：发起人指令（模拟数据须物理合理；判定通过后再用真实采集二次验证）。
//! 方法：`meta_kernel_core::l4::l4_router::route_with_baseline`（基线 = 健康态全 1.0）。

use meta_kernel_core::l4::dimension::FieldState;
use meta_kernel_core::l4::l4_gate::RejectReason;
use meta_kernel_core::l4::l4_router::route_with_baseline;
use meta_kernel_core::l4::threshold::{GOLDEN_HIGH, GOLDEN_LOW};

const OK: fn() -> FieldState = FieldState::baseline;

fn run(name: &str, s: &[f64; 7]) {
    let baseline = OK();
    let r = route_with_baseline(s, &baseline);
    let verdict = match &r {
        Ok(_) => "通过 (Pass)".to_string(),
        Err(RejectReason::UnseasonalMeal(n)) => format!("拒绝 · 不非时食 (N_high={n})"),
        Err(RejectReason::Ungrasping(n)) => format!("拒绝 · 不捉持 (N_low={n})"),
        Err(_) => "拒绝 · 格式非法".to_string(),
    };
    println!("{name}: {verdict}");
    println!("  S = ({})", s.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(", "));
}

#[test]
fn case1_idle_stable_passes() {
    // 空闲/稳定态：幅度低(0.2)、频率稳定、时间稳定——单维低能量，节律未被破坏 → 通过
    let s = [1.0, 1.0, 0.2, 1.0, 1.0, 1.0, 1.0];
    run("用例1 空闲/稳定态", &s);
    assert_eq!(route_with_baseline(&s, &OK()), Ok(s.to_vec()));
}

#[test]
fn case2_high_load_rejected() {
    // 高负载：t/f/a 三处 >1.8×（任务速率↑、调度频率偏移、能量占用↑）→ 不非时食
    let s = [2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0];
    run("用例2 高负载", &s);
    assert_eq!(route_with_baseline(&s, &OK()), Err(RejectReason::UnseasonalMeal(3)));
}

#[test]
fn case3_stall_rejected() {
    // 卡顿/停滞：时间异常(2.0>1.8×，处理延迟)、频率抖动偏移(2.0)、熵升高(2.0) 三处高偏离，
    // 相位错位(0.5<0.6×)——排队时序漂移、调度谱偏移、混乱度上升 → 不非时食
    let s = [2.0, 2.0, 1.0, 0.5, 1.0, 2.0, 1.0];
    run("用例3 卡顿/停滞", &s);
    assert_eq!(route_with_baseline(&s, &OK()), Err(RejectReason::UnseasonalMeal(3)));
}

#[test]
fn case4_resource_exhaustion_rejected() {
    // 资源枯竭：幅度(0.5)、空间分布(0.5)、熵(0.5) 三处 <0.6×（能量枯、分布收缩、活性死寂）→ 不捉持
    let s = [1.0, 1.0, 0.5, 1.0, 0.5, 0.5, 1.0];
    run("用例4 资源枯竭", &s);
    assert_eq!(route_with_baseline(&s, &OK()), Err(RejectReason::Ungrasping(3)));
}

#[test]
fn case5_mixed_deviation_passes() {
    // 混合：2 高(t,f)+2 低(x,H)+3 正常 → 无 3 个同向偏离 → 通过
    let s = [2.0, 2.0, 1.0, 1.0, 0.5, 0.5, 1.0];
    run("用例5 多偏离混合", &s);
    assert_eq!(route_with_baseline(&s, &OK()), Ok(s.to_vec()));
}

#[test]
fn case6_boundary_passes() {
    // 边界：恰等于 1.618 / 0.618（精确黄金常量）→ 不算越界 → 通过
    let s = [GOLDEN_HIGH, GOLDEN_LOW, 1.0, 1.0, 1.0, 1.0, 1.0];
    run("用例6 边界", &s);
    assert_eq!(route_with_baseline(&s, &OK()), Ok(s.to_vec()));
}

#[test]
fn no_cache_no_residue_repeatable() {
    // 无缓存、无残留：同一输入重复 5 次判定结果恒定
    let baseline = OK();
    let bad = [2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0];
    for _ in 0..5 {
        assert_eq!(route_with_baseline(&bad, &baseline), Err(RejectReason::UnseasonalMeal(3)));
    }
    let good = [1.0, 1.0, 0.2, 1.0, 1.0, 1.0, 1.0];
    for _ in 0..5 {
        assert_eq!(route_with_baseline(&good, &baseline), Ok(good.to_vec()));
    }
}
